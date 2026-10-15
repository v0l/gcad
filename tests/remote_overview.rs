use gcad::remote::{self, Overview};
use gcad_sims_proto::{self as proto, Job, JobState, Runner, RunnerInfo};
use std::io::{Read, Write};
use std::net::TcpListener;

fn job(id: &str, state: JobState, created_at: i64) -> Job {
    Job {
        id: id.into(),
        spec: "wind.gsim".into(),
        vars: Vec::new(),
        version: Overview::build().into(),
        hash: String::new(),
        state,
        progress: state.active().then(|| "step 40 of 200".to_string()),
        error: None,
        files: Vec::new(),
        max_seconds: 3600,
        price_cents_per_hour: 250,
        max_price_cents_per_hour: 250,
        runner: Some("rtx".into()),
        held_cents: if state.active() { 250 } else { 0 },
        charged_cents: (!state.active()).then_some(12),
        seconds: Some(61.0),
        queue_position: None,
        created_at,
        started_at: None,
        finished_at: None,
    }
}

fn runner(name: &str, version: &str, online: bool, price: i64) -> Runner {
    Runner {
        name: name.into(),
        version: version.into(),
        online,
        busy: false,
        last_seen: None,
        price_cents_per_hour: price,
        info: RunnerInfo {
            gpu: Some("RTX 4090".into()),
            tflops: Some(82.6),
            vram_gb: Some(24.0),
            ..Default::default()
        },
    }
}

fn serve(runners: Vec<Runner>, shop: Option<&str>) -> String {
    let shop = shop.map(str::to_string);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = [0u8; 4096];
            let n = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..n]).to_string();
            let line = request.lines().next().unwrap_or("").to_string();
            let authorised = request.contains("Bearer gsa_test");
            let (code, body) = match line.split(' ').take(2).collect::<Vec<_>>().as_slice() {
                ["GET", "/api/status"] => (
                    200,
                    serde_json::to_string(&proto::Status {
                        runners: runners.clone(),
                        versions: vec!["0.1.0".into()],
                        online: Vec::new(),
                        queued: 3,
                        shop_url: None,
                    })
                    .unwrap(),
                ),
                ["GET", "/api/account"] if authorised => (
                    200,
                    serde_json::to_string(&proto::Account {
                        id: "acct-7".into(),
                        balance_cents: 1234,
                        held_cents: 250,
                        shop_url: shop.clone(),
                    })
                    .unwrap(),
                ),
                ["GET", "/api/jobs"] if authorised => (
                    200,
                    serde_json::to_string(&proto::Jobs {
                        jobs: vec![
                            job("old00000", JobState::Succeeded, 10),
                            job("new00000", JobState::Running, 30),
                            job("mid00000", JobState::Failed, 20),
                        ],
                    })
                    .unwrap(),
                ),
                ["POST", "/api/accounts"] => (
                    200,
                    serde_json::to_string(&proto::NewAccount {
                        id: "acct-new".into(),
                        token: "gsa_test".into(),
                    })
                    .unwrap(),
                ),
                ["POST", path] if path.ends_with("/cancel") && authorised => (
                    200,
                    serde_json::to_string(&job("new00000", JobState::Cancelling, 30)).unwrap(),
                ),
                _ => (
                    402,
                    r#"{"error":"out of credit","shop_url":"https://shop.example"}"#.to_string(),
                ),
            };
            let reply = format!(
                "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    format!("http://{address}")
}

#[test]
fn the_overview_reads_account_jobs_and_runners() {
    let config = std::env::temp_dir().join(format!("gcad-remote-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&config);
    std::fs::create_dir_all(&config).expect("config dir");
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &config);
        std::env::remove_var("GCAD_SIMS_TOKEN");
    }
    let runners = vec![
        runner("old", "0.1.0", true, 0),
        runner("here", Overview::build(), true, 250),
        runner("also-here", Overview::build(), true, 100),
        runner("asleep", Overview::build(), false, 50),
    ];
    let server = serve(runners.clone(), None);
    unsafe { std::env::set_var("GCAD_SIMS_SERVER", &server) };

    assert!(!remote::has_account());
    let signed_out = remote::overview();
    assert!(!signed_out.signed_in && signed_out.account.is_none() && signed_out.jobs.is_empty());
    assert_eq!(signed_out.error, None);
    assert_eq!(signed_out.queued, 3);
    assert_eq!(signed_out.runners.len(), 4);

    let (made, made_on, path) = remote::create_account(None).expect("signup");
    assert_eq!(
        (made.id.as_str(), made_on.as_str()),
        ("acct-new", server.as_str())
    );
    assert!(path.starts_with(&config) && remote::has_account());
    let again = remote::create_account(None).expect_err("a second account");
    assert!(
        format!("{again:#}").contains("already an account"),
        "{again:#}"
    );

    let view = remote::overview();
    assert!(view.signed_in && view.error.is_none(), "{:?}", view.error);
    let account = view.account.as_ref().expect("an account");
    assert_eq!(
        (
            account.id.as_str(),
            account.balance_cents,
            account.held_cents
        ),
        ("acct-7", 1234, 250)
    );
    assert_eq!(remote::euros(account.balance_cents), "€12.34");
    let ids: Vec<&str> = view.jobs.iter().map(|j| j.id.as_str()).collect();
    assert_eq!(ids, ["new00000", "mid00000", "old00000"]);
    assert_eq!(view.active().count(), 1);
    assert_eq!(view.known_on_this_build(), 3);
    assert_eq!(view.online_on_this_build().len(), 2);
    assert_eq!(view.cheapest(), Some(100));
    assert_eq!(view.priciest(), Some(250));
    assert_eq!(view.shop(), "https://apexstrata.ie/shop/gcad-sim-credit/");
    assert_eq!(remote::hold_cents(60, 250), 250);
    assert_eq!(remote::hold_cents(20, 250), 84);

    let own = serve(runners, Some("https://shop.example/top-up"));
    unsafe { std::env::set_var("GCAD_SIMS_SERVER", &own) };
    assert_eq!(remote::overview().shop(), "https://shop.example/top-up");
    unsafe { std::env::set_var("GCAD_SIMS_SERVER", &server) };

    let cancelled = remote::cancel_job("new00000").expect("cancel");
    assert_eq!(cancelled.state, JobState::Cancelling);

    unsafe { std::env::set_var("GCAD_SIMS_SERVER", "http://127.0.0.1:9") };
    let unreachable = remote::overview();
    assert!(unreachable.signed_in);
    let error = unreachable.error.expect("an error");
    assert!(error.contains("cannot reach the sims server"), "{error}");
    let _ = std::fs::remove_dir_all(&config);
}
