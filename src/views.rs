//! HTML pages (maud). Styling: Open Props tokens + theme colors from gosrvdir; Alpine.js for filter and theme switch.

use chrono::{DateTime, Local};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

use crate::config::{Share, THEMES};
use crate::listing::{Entry, Order, SortKey, format_size};

/// Characters escaped in a path segment.
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'?')
    .add(b'/')
    .add(b'<')
    .add(b'>');

pub fn encode(segment: &str) -> String {
    utf8_percent_encode(segment, SEGMENT).to_string()
}

pub struct Site<'a> {
    pub title: &'a str,
    pub theme: &'a str,
    /// Start page with the share list exists (more than one share or config mode).
    pub has_index: bool,
}

pub struct Listing<'a> {
    pub share: &'a Share,
    /// Path inside the share without leading/trailing slash ("" = share root).
    pub rel: &'a str,
    pub entries: &'a [Entry],
    pub sort: SortKey,
    pub order: Order,
}

// Applies the saved theme before first paint (no flash).
const THEME_INIT: &str = "try{var t=localStorage.getItem('rsrvdir-theme');if(t)document.documentElement.dataset.theme=t}catch(e){}";

fn page(site: &Site, title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" data-theme=(site.theme) {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                link rel="icon" href="/_rsrvdir/logo.svg";
                link rel="stylesheet" href="/_rsrvdir/open-props.min.css";
                link rel="stylesheet" href="/_rsrvdir/style.css";
                script { (PreEscaped(THEME_INIT)) }
                script defer src="/_rsrvdir/alpine.min.js" {}
            }
            body {
                nav {
                    a.brand href="/" { img src="/_rsrvdir/logo.svg" alt="rsrvdir" height="32"; }
                    span.site-title { (site.title) }
                    (theme_switcher(site.theme))
                }
                (body)
            }
        }
    }
}

fn theme_switcher(default: &str) -> Markup {
    html! {
        div.theme-switcher x-data="{ theme: document.documentElement.dataset.theme }" {
            label for="theme-select" { "Theme" }
            select #theme-select x-model="theme"
                x-on:change="document.documentElement.dataset.theme = theme; try { localStorage.setItem('rsrvdir-theme', theme) } catch (e) {}" {
                @for (value, label) in THEMES {
                    option value=(value) selected[*value == default] { (label) }
                }
            }
        }
    }
}

pub fn index(site: &Site, shares: &[Share]) -> Markup {
    let body = html! {
        header { div.breadcrumbs { span.crumb.current { "~" } } }
        main {
            table {
                thead { tr { th.name { "Folder" } th.desc { "Description" } } }
                tbody {
                    @for s in shares {
                        tr {
                            td.name.dir { span.icon { "📁" } a href=(s.mount()) { (s.name) "/" } }
                            td.desc { (s.description) }
                        }
                    }
                }
            }
        }
    };
    page(site, site.title, body)
}

pub fn listing(site: &Site, l: &Listing) -> Markup {
    let mount = l.share.mount();
    let segments: Vec<&str> = if l.rel.is_empty() {
        vec![]
    } else {
        l.rel.split('/').collect()
    };
    let here = format!(
        "{mount}{}",
        segments.iter().map(|s| encode(s) + "/").collect::<String>()
    );
    let dirs = l.entries.iter().filter(|e| e.is_dir).count();
    let files = l.entries.len() - dirs;
    let total: u64 = l.entries.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();

    let body = html! {
        header { (breadcrumbs(site, l.share, &segments)) }
        main x-data="{ q: '' }" {
            div.toolbar {
                span.summary {
                    strong { (dirs) } (plural(dirs, " folder", " folders")) " · " strong { (files) } (plural(files, " file", " files")) " · " strong { (format_size(total)) }
                }
                input.filter type="search" placeholder="Filter" x-model="q" aria-label="Filter by name";
            }
            table {
                thead {
                    tr {
                        (sort_header(l, SortKey::Name, "Name", "name"))
                        (sort_header(l, SortKey::Size, "Size", "size"))
                        (sort_header(l, SortKey::Time, "Modified", "date"))
                    }
                }
                tbody {
                    @if !segments.is_empty() || site.has_index {
                        tr {
                            td.name.parent colspan="3" { span.icon { "⬆️" } a href=".." { ".." } }
                        }
                    }
                    @for e in l.entries {
                        @let href = format!("{here}{}{}", encode(&e.name), if e.is_dir { "/" } else { "" });
                        tr x-show="!q || $el.dataset.name.includes(q.toLowerCase())" data-name=(e.name.to_lowercase()) {
                            @if e.is_dir {
                                td.name.dir { span.icon { "📁" } a href=(href) { (e.name) "/" } }
                                td.size {}
                            } @else {
                                td.name.file {
                                    span.icon { (file_icon(&e.name)) }
                                    a href=(href) download[l.share.download] { (e.name) }
                                }
                                td.size { (format_size(e.size)) }
                            }
                            td.date { (DateTime::<Local>::from(e.modified).format("%Y-%m-%d %H:%M")) }
                        }
                    }
                    @if l.entries.is_empty() {
                        tr { td.empty colspan="3" { "Empty folder" } }
                    }
                }
            }
        }
    };
    let title = format!("{mount}{} – {}", l.rel, site.title);
    page(site, &title, body)
}

fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 { one } else { many }
}

fn breadcrumbs(site: &Site, share: &Share, segments: &[&str]) -> Markup {
    let mount = share.mount();
    let last = segments.len();
    let crumbs: Vec<(&str, String)> = segments
        .iter()
        .scan(mount.clone(), |href, s| {
            href.push_str(&encode(s));
            href.push('/');
            Some((*s, href.clone()))
        })
        .collect();
    html! {
        div.breadcrumbs {
            @if site.has_index {
                a.crumb.root href="/" { "~" }
                span.separator { "/" }
            }
            @if !share.name.is_empty() {
                @if last == 0 {
                    span.crumb.current { (share.name) }
                } @else {
                    a.crumb href=(mount) { (share.name) }
                }
                span.separator { "/" }
            }
            @for (i, (s, href)) in crumbs.iter().enumerate() {
                @if i + 1 == last {
                    span.crumb.current { (s) }
                } @else {
                    a.crumb href=(href) { (s) }
                    span.separator { "/" }
                }
            }
        }
    }
}

fn sort_header(l: &Listing, key: SortKey, label: &str, class: &str) -> Markup {
    let active = l.sort == key;
    // name sorts ascending first, size/time newest/largest first
    let order = if active {
        l.order.flip()
    } else if key == SortKey::Name {
        Order::Asc
    } else {
        Order::Desc
    };
    let arrow = match (active, l.order) {
        (false, _) => "",
        (true, Order::Asc) => " ▲",
        (true, Order::Desc) => " ▼",
    };
    html! {
        th class=(class) {
            a href={ "?sort=" (key.as_str()) "&order=" (order.as_str()) } { (label) (arrow) }
        }
    }
}

fn file_icon(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "svg" | "webp" | "ico" => "🖼️",
        "doc" | "docx" | "odt" => "📝",
        "xls" | "xlsx" | "ods" | "csv" => "📊",
        "go" | "py" | "js" | "ts" | "rs" | "c" | "cpp" | "h" | "java" | "rb" | "php" | "sh"
        | "fish" => "📜",
        "json" | "yaml" | "yml" | "toml" | "xml" | "ini" | "conf" => "⚙️",
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" => "📦",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => "🎵",
        "mp4" | "mkv" | "avi" | "mov" | "webm" => "🎬",
        "md" | "txt" | "rst" | "log" => "📃",
        "html" | "htm" | "css" => "🌐",
        _ => "📄",
    }
}
