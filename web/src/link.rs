#[derive(Debug, PartialEq)]
pub enum Link {
    Local(String),
    Github(Repo),
}

#[derive(Debug, PartialEq)]
pub struct Repo {
    pub owner: String,
    pub name: String,
    pub rev: String,
    pub path: String,
    pub folder: bool,
}

impl Repo {
    pub fn root(&self) -> String {
        format!("github/{}/{}/{}", self.owner, self.name, self.rev)
    }

    pub fn raw(&self, path: &str) -> String {
        format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{path}",
            self.owner, self.name, self.rev
        )
    }

    pub fn tree(&self) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/git/trees/{}?recursive=1",
            self.owner, self.name, self.rev
        )
    }
}

fn looks_like_file(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'))
}

pub fn parse(text: &str) -> Option<Link> {
    let text = text.trim().trim_start_matches('#').trim();
    if text.is_empty() {
        return None;
    }
    let bare = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
        .unwrap_or(text);
    let bare = bare
        .split(['?', '#'])
        .next()
        .unwrap_or(bare)
        .trim_end_matches('/');
    let github = |owner: &str, name: &str, rev: &str, path: &[&str], folder: Option<bool>| {
        let path = path.join("/");
        Some(Link::Github(Repo {
            owner: owner.to_string(),
            name: name.trim_end_matches(".git").to_string(),
            rev: if rev.is_empty() {
                "HEAD".to_string()
            } else {
                rev.to_string()
            },
            folder: folder.unwrap_or_else(|| !looks_like_file(&path)),
            path,
        }))
    };
    let parts: Vec<&str> = bare.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        ["github.com", owner, name] => github(owner, name, "", &[], Some(true)),
        ["github.com", owner, name, "blob", rev, path @ ..] => {
            github(owner, name, rev, path, Some(false))
        }
        ["github.com", owner, name, "tree", rev, path @ ..] => {
            github(owner, name, rev, path, Some(true))
        }
        ["github.com", owner, name, path @ ..] => github(owner, name, "", path, None),
        ["raw.githubusercontent.com", owner, name, rev, path @ ..] => {
            github(owner, name, rev, path, Some(false))
        }
        ["github", owner, name, rev, path @ ..] => github(owner, name, rev, path, None),
        ["examples", ..] => Some(Link::Local(parts.join("/"))),
        [owner, name, path @ ..] if !owner.contains('.') => github(owner, name, "", path, None),
        _ => Some(Link::Local(parts.join("/"))),
    }
}

pub fn hash_for(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        ["github", owner, name, rev, rest @ ..] => {
            format!("github.com/{owner}/{name}/blob/{rev}/{}", rest.join("/"))
        }
        _ => path.to_string(),
    }
}

pub const KINDS: &[&str] = &[
    "gcad", "gasm", "gsim", "step", "stp", "svg", "dxf", "stl", "ttf", "otf", "toml", "xml",
    "fcmat",
];

pub const READ_INTO: &[&str] = &["gcad", "gasm", "gsim"];

pub fn extension(path: &str) -> &str {
    let last = path.rsplit('/').next().unwrap_or(path);
    last.rsplit_once('.').map_or("", |(_, e)| e)
}

pub fn join(dir: &str, relative: &str) -> Option<String> {
    let mut out: Vec<&str> = if relative.starts_with('/') {
        Vec::new()
    } else {
        dir.split('/').filter(|p| !p.is_empty()).collect()
    };
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            other => out.push(other),
        }
    }
    Some(out.join("/"))
}

pub fn references(text: &str, dir: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for word in text.lines().flat_map(str::split_whitespace) {
        let word = word.rsplit('=').next().unwrap_or(word).trim_matches('"');
        if word.contains(':') || !KINDS.contains(&extension(word).to_ascii_lowercase().as_str()) {
            continue;
        }
        if let Some(path) = join(dir, word)
            && !found.contains(&path)
        {
            found.push(path);
        }
    }
    found
}

pub fn first_to_open<'a>(
    files: impl IntoIterator<Item = &'a String>,
    under: &str,
) -> Option<String> {
    let rank = |path: &str| {
        let rest = path
            .strip_prefix(under)
            .unwrap_or(path)
            .trim_start_matches('/');
        let kind = match extension(path) {
            "gasm" => 0,
            "gcad" => 1,
            _ => 2,
        };
        (rest.matches('/').count(), kind, rest.to_string())
    };
    files
        .into_iter()
        .filter(|p| READ_INTO.contains(&extension(p)))
        .min_by_key(|p| rank(p))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(owner: &str, name: &str, rev: &str, path: &str, folder: bool) -> Option<Link> {
        Some(Link::Github(Repo {
            owner: owner.into(),
            name: name.into(),
            rev: rev.into(),
            path: path.into(),
            folder,
        }))
    }

    #[test]
    fn parses_links() {
        assert_eq!(
            parse("#https://github.com/v0l/gcad/blob/master/examples/robot/arm.gasm"),
            repo("v0l", "gcad", "master", "examples/robot/arm.gasm", false)
        );
        assert_eq!(
            parse("github.com/v0l/gcad/tree/master/examples"),
            repo("v0l", "gcad", "master", "examples", true)
        );
        assert_eq!(
            parse("https://github.com/v0l/gcad"),
            repo("v0l", "gcad", "HEAD", "", true)
        );
        assert_eq!(
            parse("https://raw.githubusercontent.com/v0l/gcad/abc123/examples/parts/plate.gcad"),
            repo("v0l", "gcad", "abc123", "examples/parts/plate.gcad", false)
        );
        assert_eq!(
            parse("v0l/gcad/examples/parts/plate.gcad"),
            repo("v0l", "gcad", "HEAD", "examples/parts/plate.gcad", false)
        );
        assert_eq!(
            parse("#examples/parts/plate.gcad"),
            Some(Link::Local("examples/parts/plate.gcad".into()))
        );
        assert_eq!(parse("#"), None);
    }

    #[test]
    fn hash_round_trips() {
        let hash = hash_for("github/v0l/gcad/master/examples/robot/arm.gasm");
        assert_eq!(
            hash,
            "github.com/v0l/gcad/blob/master/examples/robot/arm.gasm"
        );
        assert_eq!(
            parse(&hash),
            repo("v0l", "gcad", "master", "examples/robot/arm.gasm", false)
        );
    }

    #[test]
    fn finds_references() {
        let text = "part base base.gcad\npart a ../parts/link.gcad length=40\nimport \"case.step\"\npart s shcs:M3x10\n";
        assert_eq!(
            references(text, "examples/robot"),
            [
                "examples/robot/base.gcad",
                "examples/parts/link.gcad",
                "examples/robot/case.step"
            ]
        );
        assert_eq!(join("a", "../../x.gcad"), None);
    }

    #[test]
    fn opens_the_shallowest_assembly() {
        let files: Vec<String> = [
            "examples/parts/plate.gcad",
            "examples/robot/arm.gasm",
            "examples/assemblies/case.gasm",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            first_to_open(&files, "examples").as_deref(),
            Some("examples/assemblies/case.gasm")
        );
        assert_eq!(
            first_to_open(&files, "examples/parts").as_deref(),
            Some("examples/parts/plate.gcad")
        );
    }
}
