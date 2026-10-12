use crate::link::{self, Link};
use eframe::egui;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

include!(concat!(env!("OUT_DIR"), "/bundle.rs"));

const FIRST: &str = "examples/parts/plate.gcad";

thread_local! {
    static LATEST: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static CONTEXT: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
}

fn repaint() {
    CONTEXT.with(|c| {
        if let Some(ctx) = c.borrow().as_ref() {
            ctx.request_repaint();
        }
    });
}

fn hash() -> String {
    let raw = web_sys::window()
        .and_then(|w| w.location().hash().ok())
        .unwrap_or_default();
    js_sys::decode_uri_component(&raw)
        .ok()
        .and_then(|s| s.as_string())
        .unwrap_or(raw)
}

fn query_file() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search)
        .ok()?
        .get("file")
}

fn show_in_address_bar(path: &Path) {
    LATEST.with(|n| n.set(n.get() + 1));
    let hash = format!("#{}", link::hash_for(&path.to_string_lossy()));
    let Some(window) = web_sys::window() else {
        return;
    };
    if window.location().hash().ok().as_deref() == Some(hash.as_str()) {
        return;
    }
    if let Ok(history) = window.history() {
        let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&hash));
    }
}

fn follow(target: Option<Link>) -> Option<PathBuf> {
    let asked = LATEST.with(|n| {
        n.set(n.get() + 1);
        n.get()
    });
    match target {
        None => None,
        Some(Link::Local(path)) => {
            let path = PathBuf::from(path);
            if gcad::host::is_file(&path) {
                Some(path)
            } else {
                gcad::host::set_status(Some(format!("no file {}", path.display())));
                None
            }
        }
        Some(Link::Github(repo)) => {
            let label = format!(
                "fetching github.com/{}/{} {}",
                repo.owner, repo.name, repo.path
            );
            gcad::host::set_status(Some(label));
            repaint();
            wasm_bindgen_futures::spawn_local(async move {
                let result = crate::fetch::open(repo).await;
                if LATEST.with(|n| n.get()) != asked {
                    return;
                }
                match result {
                    Ok(path) => {
                        gcad::host::set_status(None);
                        gcad::host::ask_open(Path::new(&path));
                    }
                    Err(error) => gcad::host::set_status(Some(error)),
                }
                repaint();
            });
            None
        }
    }
}

fn listen_for_hash_changes() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let changed = Closure::<dyn FnMut()>::new(|| {
        if let Some(path) = follow(link::parse(&hash())) {
            gcad::host::ask_open(&path);
            repaint();
        }
    });
    let _ = window.add_event_listener_with_callback("hashchange", changed.as_ref().unchecked_ref());
    changed.forget();
}

fn note_build_time(seconds: f64) {
    if let Some(window) = web_sys::window() {
        let _ = js_sys::Reflect::set(&window, &"gcadBuilt".into(), &seconds.into());
    }
}

#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();
    for (path, bytes) in FILES {
        gcad::host::bundle(Path::new(path), bytes);
    }
    gcad::host::restore();
    gcad::host::on_open(show_in_address_bar);
    gcad::host::on_built(note_build_time);
    let asked = link::parse(&hash()).or_else(|| query_file().and_then(|f| link::parse(&f)));
    let file = match asked {
        Some(target) => follow(Some(target)),
        None => Some(PathBuf::from(FIRST)),
    };
    listen_for_hash_changes();
    wasm_bindgen_futures::spawn_local(async move {
        let canvas = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("gcad"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("the page has no <canvas id=\"gcad\">");
        let options = eframe::WebOptions {
            depth_buffer: 24,
            ..Default::default()
        };
        let started = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(move |cc| {
                    CONTEXT.with(|c| *c.borrow_mut() = Some(cc.egui_ctx.clone()));
                    Ok(Box::new(gcad::view::App::web(cc, file)))
                }),
            )
            .await;
        if let Err(error) = started {
            web_sys::console::error_1(&error);
        }
    });
}
