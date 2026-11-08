//! The server rail's order and folders, in the web client's two synced
//! user settings, so both clients arrange the rail the same way.
//!
//! - `ordering`: `{ servers: [id], serverSidebar?: [id] }`. `servers` is the
//!   legacy order, read only: it's where the rail starts until
//!   `serverSidebar` (server and folder IDs, each folder's followed by its
//!   servers') is written. A write keeps `servers` as it was.
//! - `server-folders`: `{ folders: [{ id, name, colour?, collapsed?,
//!   servers }] }`. The folders' own order means nothing: `serverSidebar`
//!   places them (or else their first server does).
//!
//! Both are kept with their unknown fields, so a write here loses nothing
//! the web client (or a later one) stores in them.
//!
//! The menus' edits to folders are here too, each on the two settings as
//! held and the rail as drawn from them.

use std::collections::{HashMap, HashSet};

use gpui_kit::{Rgba, rgb};
use serde_json::{Map, Value};

/// The settings' keys.
pub const ORDERING_KEY: &str = "ordering";
pub const FOLDERS_KEY: &str = "server-folders";

const SERVERS: &str = "servers";
const SERVER_SIDEBAR: &str = "serverSidebar";
const FOLDERS: &str = "folders";
const ID: &str = "id";
const NAME: &str = "name";
const COLOUR: &str = "colour";
const COLLAPSED: &str = "collapsed";

/// What a folder made from the server menu is called, as in the web client.
pub const NEW_FOLDER_NAME: &str = "New Folder";

/// The colours the Edit folder dialog offers, as (name, hex), chosen to
/// read on light and dark themes. A folder may hold another colour, set in
/// the web client, which is drawn if [`parse_colour`] can read it.
pub const PALETTE: [(&str, &str); 12] = [
    ("Red", "#E5484D"),
    ("Orange", "#F76B15"),
    ("Amber", "#FFB224"),
    ("Yellow", "#F5D90A"),
    ("Lime", "#99D52A"),
    ("Green", "#30A46C"),
    ("Teal", "#12A594"),
    ("Cyan", "#05A2C2"),
    ("Blue", "#3E63DD"),
    ("Indigo", "#6E56CF"),
    ("Pink", "#D6409F"),
    ("Grey", "#8B8D98"),
];

/// The `ordering` user setting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ordering {
    /// The legacy order, cleaned. `raw` keeps it as stored, to write back.
    servers: Vec<String>,
    /// The rail's order, if a client that knows folders has written one.
    server_sidebar: Option<Vec<String>>,
    /// The setting as parsed, less `serverSidebar`.
    raw: Map<String, Value>,
}

impl Ordering {
    /// Parse the setting's JSON. Lenient like the web client's `clean()`:
    /// anything malformed reads as no order at all.
    pub fn parse(json: &str) -> Self {
        let mut raw = match serde_json::from_str(json) {
            Ok(Value::Object(raw)) => raw,
            _ => Map::new(),
        };
        let servers = raw.get(SERVERS).map(clean_ids).unwrap_or_default();
        let server_sidebar = raw
            .remove(SERVER_SIDEBAR)
            .filter(Value::is_array)
            .map(|ids| clean_ids(&ids));
        Ordering {
            servers,
            server_sidebar,
            raw,
        }
    }

    /// The legacy order.
    pub fn servers(&self) -> &[String] {
        &self.servers
    }

    /// The rail's order, if one has been written.
    pub fn server_sidebar(&self) -> Option<&[String]> {
        self.server_sidebar.as_deref()
    }

    /// The order the rail is drawn in: `serverSidebar`, else `servers`.
    pub fn sidebar(&self) -> &[String] {
        self.server_sidebar().unwrap_or(&self.servers)
    }

    /// Set the rail's order (see [`to_sidebar`]). `servers` stays as it was.
    pub fn set_server_sidebar(&mut self, ids: Vec<String>) {
        self.server_sidebar = Some(clean_ids(&Value::from(ids)));
    }

    /// The setting as written: `raw`, with `serverSidebar` if there is one.
    /// `servers` is always there, as the web client writes it.
    pub fn to_json(&self) -> String {
        let mut raw = self.raw.clone();
        raw.entry(SERVERS)
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(sidebar) = &self.server_sidebar {
            raw.insert(SERVER_SIDEBAR.into(), sidebar.clone().into());
        }
        Value::Object(raw).to_string()
    }
}

/// The `server-folders` user setting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerFolders {
    /// In no particular order. A server is in one folder at most.
    pub folders: Vec<Folder>,
    /// The setting as parsed, less `folders`.
    raw: Map<String, Value>,
}

/// A folder of servers, drawn as one item in the rail.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folder {
    /// `folder-` and a ULID, so it can't be taken for a server's.
    pub id: String,
    /// Possibly empty: the rail then says "Folder".
    pub name: String,
    /// Any CSS colour, as the web client allows.
    pub colour: Option<String>,
    pub collapsed: bool,
    /// In the order they're drawn in.
    pub servers: Vec<String>,
    /// The folder as parsed, less the fields above.
    raw: Map<String, Value>,
}

impl ServerFolders {
    /// Parse the setting's JSON. Lenient like the web client's `clean()`:
    /// folders without an ID (or a repeated one) are dropped, and so is a
    /// server's place in any folder after the first that has it.
    pub fn parse(json: &str) -> Self {
        let mut raw = match serde_json::from_str(json) {
            Ok(Value::Object(raw)) => raw,
            _ => Map::new(),
        };
        let mut folders = Vec::new();
        let mut seen_folders = HashSet::new();
        let mut seen_servers = HashSet::new();
        let listed = match raw.remove(FOLDERS) {
            Some(Value::Array(listed)) => listed,
            _ => Vec::new(),
        };
        for folder in listed {
            let Value::Object(mut folder) = folder else {
                continue;
            };
            let id = match folder.remove(ID) {
                Some(Value::String(id)) if !id.is_empty() && seen_folders.insert(id.clone()) => id,
                _ => continue,
            };
            let servers = match folder.remove(SERVERS) {
                Some(Value::Array(servers)) => servers
                    .into_iter()
                    .filter_map(|id| match id {
                        Value::String(id) if seen_servers.insert(id.clone()) => Some(id),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let name = match folder.remove(NAME) {
                Some(Value::String(name)) => name,
                _ => String::new(),
            };
            let colour = match folder.remove(COLOUR) {
                Some(Value::String(colour)) => Some(colour),
                _ => None,
            };
            let collapsed = folder.remove(COLLAPSED) == Some(Value::Bool(true));
            folders.push(Folder {
                id,
                name,
                colour,
                collapsed,
                servers,
                raw: folder,
            });
        }
        ServerFolders { folders, raw }
    }

    /// The folder `id`, if there is one.
    pub fn folder(&self, id: &str) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == id)
    }

    /// The folder `server` is in, if any.
    pub fn folder_of(&self, server: &str) -> Option<&Folder> {
        self.folders
            .iter()
            .find(|f| f.servers.iter().any(|s| s == server))
    }

    fn folder_mut(&mut self, id: &str) -> Option<&mut Folder> {
        self.folders.iter_mut().find(|f| f.id == id)
    }

    /// Rename the folder `id` (an empty name is allowed: it's "Folder").
    pub fn rename_folder(&mut self, id: &str, name: &str) {
        if let Some(folder) = self.folder_mut(id) {
            folder.name = name.to_string();
        }
    }

    /// Set the folder `id`'s colour, or take it away.
    pub fn recolour_folder(&mut self, id: &str, colour: Option<&str>) {
        if let Some(folder) = self.folder_mut(id) {
            folder.colour = colour.map(str::to_string);
        }
    }

    /// Close the folder `id` if it's open, else open it.
    pub fn toggle_folder(&mut self, id: &str) {
        if let Some(folder) = self.folder_mut(id) {
            folder.collapsed = !folder.collapsed;
        }
    }

    /// Take `server` out of whichever folder holds it.
    fn take_out(&mut self, server: &str) {
        for folder in &mut self.folders {
            folder.servers.retain(|s| s != server);
        }
    }

    /// The setting as written. Folders left without servers are dropped,
    /// as the web client drops them; `colour` is written only if there is
    /// one, and `collapsed` only if true.
    pub fn to_json(&self) -> String {
        let folders: Vec<Value> = self
            .folders
            .iter()
            .filter(|f| !f.servers.is_empty())
            .map(|f| {
                let mut folder = f.raw.clone();
                folder.insert(ID.into(), f.id.clone().into());
                folder.insert(NAME.into(), f.name.clone().into());
                if let Some(colour) = &f.colour {
                    folder.insert(COLOUR.into(), colour.clone().into());
                }
                if f.collapsed {
                    folder.insert(COLLAPSED.into(), true.into());
                }
                folder.insert(SERVERS.into(), f.servers.clone().into());
                Value::Object(folder)
            })
            .collect();
        let mut raw = self.raw.clone();
        raw.insert(FOLDERS.into(), folders.into());
        Value::Object(raw).to_string()
    }
}

impl Folder {
    /// A new, open folder of `servers`, with no colour.
    pub fn new(id: String, name: &str, servers: Vec<String>) -> Self {
        Folder {
            id,
            name: name.to_string(),
            servers,
            ..Folder::default()
        }
    }
}

/// A new folder's ID: `folder-` and a ULID, as the web client makes them,
/// so it can't be taken for a server's.
pub fn new_folder_id() -> String {
    format!("folder-{}", ulid::Ulid::generate())
}

/// One item in the rail, top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RailEntry {
    Server(String),
    /// A folder and its servers that the user is in, in order (never none).
    Folder {
        folder: Folder,
        servers: Vec<String>,
    },
}

impl RailEntry {
    /// The entry's servers: the one, or the folder's.
    pub fn servers(&self) -> &[String] {
        match self {
            RailEntry::Server(id) => std::slice::from_ref(id),
            RailEntry::Folder { servers, .. } => servers,
        }
    }
}

/// The rail's items, as the web client's `orderedEntries` works them out.
/// `known` is the user's servers, in `Ready` order with any joined since
/// after them.
///
/// Each ID in [`Ordering::sidebar`] is a folder (by its ID, or one of its
/// servers'), drawn where it first comes, or a server. Servers it doesn't
/// list come last, in `known` order (so new ones go at the bottom), and
/// may bring their folder with them. IDs of servers the user isn't in are
/// skipped, and folders with none they're in aren't drawn.
pub fn entries(ordering: &Ordering, folders: &ServerFolders, known: &[String]) -> Vec<RailEntry> {
    let by_id: HashMap<&str, &Folder> =
        folders.folders.iter().map(|f| (f.id.as_str(), f)).collect();
    let mut folder_of: HashMap<&str, &Folder> = HashMap::new();
    for folder in &folders.folders {
        for server in &folder.servers {
            folder_of.entry(server.as_str()).or_insert(folder);
        }
    }

    let mut unplaced: HashSet<&str> = known.iter().map(String::as_str).collect();
    let mut drawn = HashSet::new();
    let mut out = Vec::new();
    for id in ordering.sidebar().iter().chain(known) {
        let folder = by_id
            .get(id.as_str())
            .or_else(|| folder_of.get(id.as_str()));
        if let Some(folder) = folder {
            if !drawn.insert(folder.id.as_str()) {
                continue;
            }
            let servers: Vec<String> = folder
                .servers
                .iter()
                .filter(|s| unplaced.remove(s.as_str()))
                .cloned()
                .collect();
            if !servers.is_empty() {
                out.push(RailEntry::Folder {
                    folder: (*folder).clone(),
                    servers,
                });
            }
        } else if unplaced.remove(id.as_str()) {
            out.push(RailEntry::Server(id.clone()));
        }
    }
    out
}

/// `serverSidebar` for the rail `entries`: each folder's ID followed by its
/// servers', so they keep their place if it's deleted.
pub fn to_sidebar(entries: &[RailEntry]) -> Vec<String> {
    entries
        .iter()
        .flat_map(|entry| match entry {
            RailEntry::Server(id) => vec![id.clone()],
            RailEntry::Folder { folder, servers } => std::iter::once(&folder.id)
                .chain(servers)
                .cloned()
                .collect(),
        })
        .collect()
}

// The edits below that move servers between folders first write the rail
// as drawn (`rail`, from `entries`) into `serverSidebar`, so nothing else
// moves: a folder placed by its first server, or a server listed apart
// from its folder, would otherwise jump once the folders change.

/// Make a folder ([`NEW_FOLDER_NAME`], `id`) of `server`, in its place,
/// taking it out of any folder it was in.
pub fn new_folder_with(
    ordering: &mut Ordering,
    folders: &mut ServerFolders,
    rail: &[RailEntry],
    server: &str,
    id: String,
) {
    let mut sidebar = to_sidebar(rail);
    folders.take_out(server);
    folders.folders.push(Folder::new(
        id.clone(),
        NEW_FOLDER_NAME,
        vec![server.to_string()],
    ));
    match sidebar.iter().position(|s| s == server) {
        Some(at) => sidebar.insert(at, id),
        None => sidebar.extend([id, server.to_string()]),
    }
    settle(ordering, folders, sidebar);
}

/// Move `server` to the end of the folder `folder`, taking it out of any
/// other. Nothing if the folder's gone or already has it.
pub fn add_to_folder(
    ordering: &mut Ordering,
    folders: &mut ServerFolders,
    rail: &[RailEntry],
    server: &str,
    folder: &str,
) {
    let has = folders
        .folder(folder)
        .map(|f| f.servers.iter().any(|s| s == server));
    if has != Some(false) {
        return;
    }
    let mut sidebar = to_sidebar(rail);
    sidebar.retain(|s| s != server);
    folders.take_out(server);
    let Some(target) = folders.folder_mut(folder) else {
        return;
    };
    target.servers.push(server.to_string());
    // After the folder's servers, so the folder stays where it is.
    match sidebar.iter().position(|s| s == folder) {
        Some(at) => {
            let end = sidebar[at + 1..]
                .iter()
                .position(|s| !target.servers.contains(s))
                .map_or(sidebar.len(), |n| at + 1 + n);
            sidebar.insert(end, server.to_string());
        }
        None => sidebar.extend([folder.to_string(), server.to_string()]),
    }
    settle(ordering, folders, sidebar);
}

/// Take `server` out of its folder: it's drawn just below the folder (or
/// in its place, if it was the last in it). Nothing if it's in none.
pub fn remove_from_folder(
    ordering: &mut Ordering,
    folders: &mut ServerFolders,
    rail: &[RailEntry],
    server: &str,
) {
    if folders.folder_of(server).is_none() {
        return;
    }
    let sidebar = to_sidebar(rail);
    folders.take_out(server);
    settle(ordering, folders, sidebar);
}

/// Delete the folder `id`. Its servers stay where they are, out of it.
pub fn delete_folder(
    ordering: &mut Ordering,
    folders: &mut ServerFolders,
    rail: &[RailEntry],
    id: &str,
) {
    if folders.folder(id).is_none() {
        return;
    }
    let mut sidebar = to_sidebar(rail);
    sidebar.retain(|s| s != id);
    folders.folders.retain(|f| f.id != id);
    settle(ordering, folders, sidebar);
}

/// Drop the folders left with no servers (as writing them would), and
/// their place in `sidebar`; then make `sidebar` the rail's order.
fn settle(ordering: &mut Ordering, folders: &mut ServerFolders, mut sidebar: Vec<String>) {
    let empty: HashSet<String> = folders
        .folders
        .iter()
        .filter(|f| f.servers.is_empty())
        .map(|f| f.id.clone())
        .collect();
    folders.folders.retain(|f| !f.servers.is_empty());
    sidebar.retain(|s| !empty.contains(s));
    ordering.set_server_sidebar(sidebar);
}

/// A folder's colour as drawn: `#rgb` or `#rrggbb`. Anything else the web
/// client allows (a CSS name, `rgb()`) isn't drawn.
pub fn parse_colour(colour: &str) -> Option<Rgba> {
    let digits = colour.trim().strip_prefix('#')?;
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digits = match digits.len() {
        3 => digits.chars().flat_map(|c| [c, c]).collect(),
        6 => digits.to_string(),
        _ => return None,
    };
    u32::from_str_radix(&digits, 16).ok().map(rgb)
}

/// A list of IDs as the web client cleans one: the strings, with a
/// folder's (an object with `servers`, from an early web client) standing
/// for its servers, each kept the first time it comes.
fn clean_ids(list: &Value) -> Vec<String> {
    let mut seen = HashSet::new();
    list.as_array()
        .into_iter()
        .flatten()
        .flat_map(|entry| match entry {
            Value::String(_) => std::slice::from_ref(entry),
            _ => entry
                .get(SERVERS)
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice),
        })
        .filter_map(Value::as_str)
        .filter(|id| !id.is_empty() && seen.insert(*id))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn server(id: &str) -> RailEntry {
        RailEntry::Server(id.into())
    }

    /// The rail as (entry ID, its servers): a server's own ID, or a
    /// folder's and its servers'.
    fn shape(entries: &[RailEntry]) -> Vec<(String, Vec<String>)> {
        entries
            .iter()
            .map(|entry| match entry {
                RailEntry::Server(id) => (id.clone(), vec![id.clone()]),
                RailEntry::Folder { folder, servers } => (folder.id.clone(), servers.clone()),
            })
            .collect()
    }

    fn folders(value: Value) -> ServerFolders {
        ServerFolders::parse(&value.to_string())
    }

    fn ordering(value: Value) -> Ordering {
        Ordering::parse(&value.to_string())
    }

    #[test]
    fn the_legacy_order_places_servers_until_there_is_a_sidebar() {
        let ordering = ordering(json!({ "servers": ["b", "a"] }));
        assert_eq!(ordering.server_sidebar(), None);
        let entries = entries(&ordering, &ServerFolders::default(), &ids(&["a", "b"]));
        assert_eq!(entries, vec![server("b"), server("a")]);
    }

    #[test]
    fn the_sidebar_wins_over_the_legacy_order() {
        let ordering =
            ordering(json!({ "servers": ["b", "a", "c"], "serverSidebar": ["c", "a", "b"] }));
        let entries = entries(&ordering, &ServerFolders::default(), &ids(&["a", "b", "c"]));
        assert_eq!(entries, vec![server("c"), server("a"), server("b")]);
    }

    #[test]
    fn a_folder_goes_where_its_id_is_or_else_where_its_first_server_is() {
        let ordering = ordering(
            json!({ "servers": [], "serverSidebar": ["a", "folder-1", "b", "c", "d", "e"] }),
        );
        let folders = folders(json!({ "folders": [
            { "id": "folder-1", "name": "One", "servers": ["c", "b"] },
            // Not listed itself: it goes where "e" is.
            { "id": "folder-2", "name": "Two", "servers": ["e", "d"] }
        ] }));
        let entries = entries(&ordering, &folders, &ids(&["a", "b", "c", "d", "e"]));
        assert_eq!(
            shape(&entries),
            vec![
                ("a".into(), ids(&["a"])),
                // Its servers are drawn in its own order.
                ("folder-1".into(), ids(&["c", "b"])),
                ("folder-2".into(), ids(&["e", "d"])),
            ]
        );
        let RailEntry::Folder { folder, .. } = &entries[1] else {
            panic!("a folder");
        };
        assert_eq!(folder.name, "One");
    }

    #[test]
    fn unknown_ids_are_skipped_and_unlisted_servers_come_last_in_ready_order() {
        let ordering = ordering(json!({ "servers": ["gone", "b", "folder-gone"] }));
        let entries = entries(&ordering, &ServerFolders::default(), &ids(&["c", "a", "b"]));
        assert_eq!(entries, vec![server("b"), server("c"), server("a")]);
    }

    #[test]
    fn an_unlisted_server_brings_its_folder_to_the_end() {
        let ordering = ordering(json!({ "servers": ["a"] }));
        let folders = folders(json!({ "folders": [{ "id": "folder-1", "servers": ["b", "c"] }] }));
        let entries = entries(&ordering, &folders, &ids(&["c", "a", "b"]));
        assert_eq!(
            shape(&entries),
            vec![
                ("a".into(), ids(&["a"])),
                ("folder-1".into(), ids(&["b", "c"]))
            ]
        );
    }

    #[test]
    fn a_server_two_folders_claim_goes_to_the_first() {
        let folders = folders(json!({ "folders": [
            { "id": "folder-1", "servers": ["a", "b"] },
            { "id": "folder-2", "servers": ["b", "c"] }
        ] }));
        assert_eq!(folders.folders[1].servers, ids(&["c"]));
        let entries = entries(&Ordering::default(), &folders, &ids(&["a", "b", "c"]));
        assert_eq!(
            shape(&entries),
            vec![
                ("folder-1".into(), ids(&["a", "b"])),
                ("folder-2".into(), ids(&["c"]))
            ]
        );
    }

    #[test]
    fn a_folder_without_known_servers_is_not_drawn() {
        let ordering =
            ordering(json!({ "servers": [], "serverSidebar": ["folder-1", "a", "folder-2", "a"] }));
        let folders = folders(json!({ "folders": [
            { "id": "folder-1", "servers": ["left"] },
            { "id": "folder-2", "servers": [] }
        ] }));
        let entries = entries(&ordering, &folders, &ids(&["a"]));
        assert_eq!(entries, vec![server("a")]);
    }

    #[test]
    fn repeated_ids_count_once() {
        let ordering = ordering(json!({ "servers": ["a", "b", "a", "", 7] }));
        assert_eq!(ordering.servers(), ids(&["a", "b"]));
        let entries = entries(&ordering, &ServerFolders::default(), &ids(&["b", "a"]));
        assert_eq!(entries, vec![server("a"), server("b")]);
    }

    #[test]
    fn an_early_folder_in_the_order_stands_for_its_servers() {
        let ordering = ordering(json!({ "servers": ["a", { "id": "f", "servers": ["c", "b"] }] }));
        assert_eq!(ordering.servers(), ids(&["a", "c", "b"]));
    }

    #[test]
    fn malformed_settings_read_as_empty() {
        for json in [
            "",
            "not json",
            "[]",
            "{\"servers\": 3}",
            "{\"serverSidebar\": {}}",
        ] {
            let ordering = Ordering::parse(json);
            assert!(ordering.servers().is_empty(), "{json}");
            assert_eq!(ordering.server_sidebar(), None, "{json}");
        }
        for json in [
            "",
            "{\"folders\": {}}",
            "{\"folders\": [3, {\"name\": \"no id\"}, {\"id\": \"\"}]}",
        ] {
            assert!(ServerFolders::parse(json).folders.is_empty(), "{json}");
        }
    }

    #[test]
    fn the_web_clients_ordering_survives_a_write() {
        let mut ordering = ordering(json!({
            "servers": ["b", "a"],
            "serverSidebar": ["folder-1", "a", "b"],
            "future": { "kept": true }
        }));
        ordering.set_server_sidebar(ids(&["b", "folder-1", "a", "b"]));
        let written: Value = serde_json::from_str(&ordering.to_json()).unwrap();
        assert_eq!(
            written,
            json!({
                // The legacy order is left as older clients last saw it.
                "servers": ["b", "a"],
                "serverSidebar": ["b", "folder-1", "a"],
                "future": { "kept": true }
            })
        );
        assert_eq!(Ordering::parse(&ordering.to_json()), ordering);
    }

    #[test]
    fn a_write_keeps_servers_even_where_there_was_none() {
        let mut ordering = Ordering::default();
        ordering.set_server_sidebar(ids(&["a"]));
        let written: Value = serde_json::from_str(&ordering.to_json()).unwrap();
        assert_eq!(written, json!({ "servers": [], "serverSidebar": ["a"] }));
    }

    #[test]
    fn the_web_clients_folders_survive_a_write() {
        let original = json!({
            "folders": [
                {
                    "id": "folder-1", "name": "Games", "colour": "#E5484D",
                    "collapsed": true, "servers": ["a", "b"], "icon": "🎮"
                },
                { "id": "folder-2", "name": "", "collapsed": false, "servers": ["c"] },
                { "id": "folder-empty", "name": "Left", "servers": [] }
            ],
            "future": 1
        });
        let folders = folders(original);
        let games = folders.folder("folder-1").unwrap();
        assert_eq!(games.colour.as_deref(), Some("#E5484D"));
        assert!(games.collapsed);
        assert!(!folders.folder("folder-2").unwrap().collapsed);
        let written: Value = serde_json::from_str(&folders.to_json()).unwrap();
        assert_eq!(
            written,
            json!({
                "folders": [
                    {
                        "id": "folder-1", "name": "Games", "colour": "#E5484D",
                        "collapsed": true, "servers": ["a", "b"], "icon": "🎮"
                    },
                    // `collapsed` only when true; empty folders dropped.
                    { "id": "folder-2", "name": "", "servers": ["c"] }
                ],
                "future": 1
            })
        );
        let reread = ServerFolders::parse(&folders.to_json());
        assert_eq!(ServerFolders::parse(&reread.to_json()), reread);
    }

    #[test]
    fn the_sidebar_lists_each_folder_before_its_servers() {
        let folders = folders(json!({ "folders": [{ "id": "folder-1", "servers": ["b", "c"] }] }));
        let entries = entries(&Ordering::default(), &folders, &ids(&["a", "b", "c", "d"]));
        assert_eq!(to_sidebar(&entries), ids(&["a", "folder-1", "b", "c", "d"]));
        // And it reads back as the same rail.
        let mut ordering = Ordering::default();
        ordering.set_server_sidebar(to_sidebar(&entries));
        assert_eq!(
            super::entries(&ordering, &folders, &ids(&["d", "c", "b", "a"])),
            entries
        );
    }

    /// `edit` applied to the rail drawn from `ordering` and `folders` for
    /// the servers `known`: the settings as written and read back, and the
    /// rail drawn from them.
    fn after(
        ordering: &Ordering,
        folders: &ServerFolders,
        known: &[String],
        edit: impl FnOnce(&mut Ordering, &mut ServerFolders, &[RailEntry]),
    ) -> (Ordering, ServerFolders, Vec<RailEntry>) {
        let rail = entries(ordering, folders, known);
        let (mut ordering, mut folders) = (ordering.clone(), folders.clone());
        edit(&mut ordering, &mut folders, &rail);
        let ordering = Ordering::parse(&ordering.to_json());
        let folders = ServerFolders::parse(&folders.to_json());
        let rail = entries(&ordering, &folders, known);
        (ordering, folders, rail)
    }

    #[test]
    fn a_new_folder_takes_its_servers_place() {
        let known = ids(&["a", "b", "c"]);
        let (ordering, folders, rail) = after(
            &Ordering::default(),
            &ServerFolders::default(),
            &known,
            |o, f, rail| new_folder_with(o, f, rail, "b", "folder-new".into()),
        );
        assert_eq!(
            shape(&rail),
            vec![
                ("a".into(), ids(&["a"])),
                ("folder-new".into(), ids(&["b"])),
                ("c".into(), ids(&["c"])),
            ]
        );
        assert_eq!(
            ordering.server_sidebar(),
            Some(&ids(&["a", "folder-new", "b", "c"])[..])
        );
        let folder = folders.folder("folder-new").unwrap();
        assert_eq!(folder.name, "New Folder");
        assert_eq!((folder.colour.as_deref(), folder.collapsed), (None, false));
    }

    #[test]
    fn a_new_folder_takes_its_server_from_another() {
        let known = ids(&["a", "b", "c"]);
        let folders = folders(json!({ "folders": [
            { "id": "folder-1", "servers": ["a", "b"] },
            { "id": "folder-2", "servers": ["c"] }
        ] }));
        let (_, _, rail) = after(&Ordering::default(), &folders, &known, |o, f, rail| {
            new_folder_with(o, f, rail, "b", "folder-new".into())
        });
        assert_eq!(
            shape(&rail),
            vec![
                ("folder-1".into(), ids(&["a"])),
                ("folder-new".into(), ids(&["b"])),
                ("folder-2".into(), ids(&["c"])),
            ]
        );
        // Its last server gone, the old folder goes too.
        let (ordering, folders, rail) =
            after(&Ordering::default(), &folders, &known, |o, f, rail| {
                new_folder_with(o, f, rail, "c", "folder-new".into())
            });
        assert_eq!(folders.folder("folder-2"), None);
        assert!(!ordering.sidebar().contains(&"folder-2".to_string()));
        assert_eq!(rail.last().map(|e| e.servers().to_vec()), Some(ids(&["c"])));
    }

    #[test]
    fn adding_to_a_folder_leaves_the_folder_where_it_is() {
        let known = ids(&["a", "b", "c", "d"]);
        let ordering =
            ordering(json!({ "servers": [], "serverSidebar": ["a", "b", "folder-1", "c"] }));
        let folders = folders(json!({ "folders": [{ "id": "folder-1", "servers": ["c"] }] }));
        let (ordering, folders, rail) = after(&ordering, &folders, &known, |o, f, rail| {
            add_to_folder(o, f, rail, "a", "folder-1")
        });
        assert_eq!(
            shape(&rail),
            vec![
                ("b".into(), ids(&["b"])),
                // At the end of it.
                ("folder-1".into(), ids(&["c", "a"])),
                ("d".into(), ids(&["d"])),
            ]
        );
        assert_eq!(
            ordering.server_sidebar(),
            Some(&ids(&["b", "folder-1", "c", "a", "d"])[..])
        );
        assert_eq!(
            folders.folder_of("a").map(|f| f.id.as_str()),
            Some("folder-1")
        );
    }

    #[test]
    fn adding_a_server_in_another_folder_moves_it() {
        let known = ids(&["a", "b", "c"]);
        let folders = folders(json!({ "folders": [
            { "id": "folder-1", "servers": ["a", "b"] },
            { "id": "folder-2", "servers": ["c"] }
        ] }));
        let (_, folders, rail) = after(&Ordering::default(), &folders, &known, |o, f, rail| {
            add_to_folder(o, f, rail, "b", "folder-2")
        });
        assert_eq!(
            shape(&rail),
            vec![
                ("folder-1".into(), ids(&["a"])),
                ("folder-2".into(), ids(&["c", "b"])),
            ]
        );
        assert_eq!(folders.folder("folder-1").unwrap().servers, ids(&["a"]));
    }

    #[test]
    fn adding_to_its_own_folder_or_a_gone_one_changes_nothing() {
        let ordering = ordering(json!({ "servers": ["b", "a"] }));
        let folders = folders(json!({ "folders": [{ "id": "folder-1", "servers": ["a"] }] }));
        let rail = entries(&ordering, &folders, &ids(&["a", "b"]));
        for folder in ["folder-1", "folder-gone"] {
            let (mut edited, mut edited_folders) = (ordering.clone(), folders.clone());
            add_to_folder(&mut edited, &mut edited_folders, &rail, "a", folder);
            assert_eq!(
                (edited, edited_folders),
                (ordering.clone(), folders.clone())
            );
        }
    }

    #[test]
    fn a_server_taken_out_of_its_folder_goes_just_below_it() {
        let known = ids(&["a", "b", "c", "d"]);
        let folders =
            folders(json!({ "folders": [{ "id": "folder-1", "servers": ["a", "b", "c"] }] }));
        let (_, folders, rail) = after(&Ordering::default(), &folders, &known, |o, f, rail| {
            remove_from_folder(o, f, rail, "a")
        });
        assert_eq!(
            shape(&rail),
            vec![
                ("folder-1".into(), ids(&["b", "c"])),
                ("a".into(), ids(&["a"])),
                ("d".into(), ids(&["d"])),
            ]
        );
        assert_eq!(folders.folder_of("a"), None);
    }

    #[test]
    fn taking_out_a_folders_last_server_drops_the_folder() {
        let known = ids(&["a", "b", "c"]);
        let ordering =
            ordering(json!({ "servers": [], "serverSidebar": ["a", "folder-1", "b", "c"] }));
        let folders = folders(json!({ "folders": [{ "id": "folder-1", "servers": ["b"] }] }));
        let (ordering, folders, rail) = after(&ordering, &folders, &known, |o, f, rail| {
            remove_from_folder(o, f, rail, "b")
        });
        assert_eq!(rail, vec![server("a"), server("b"), server("c")]);
        assert!(folders.folders.is_empty());
        assert_eq!(ordering.server_sidebar(), Some(&ids(&["a", "b", "c"])[..]));
        // A server in no folder: nothing to do.
        let mut unchanged = (ordering.clone(), folders.clone());
        remove_from_folder(&mut unchanged.0, &mut unchanged.1, &rail, "a");
        assert_eq!(unchanged, (ordering, folders));
    }

    #[test]
    fn a_deleted_folders_servers_stay_where_they_were() {
        let known = ids(&["x", "a", "y", "b"]);
        // Placed by its first server, with its second listed apart from it.
        let ordering = ordering(json!({ "servers": [], "serverSidebar": ["x", "a", "y", "b"] }));
        let folders = folders(json!({ "folders": [
            { "id": "folder-1", "servers": ["a", "b"] },
            { "id": "folder-2", "servers": ["y"] }
        ] }));
        assert_eq!(
            shape(&entries(&ordering, &folders, &known))[1],
            ("folder-1".into(), ids(&["a", "b"]))
        );
        let (ordering, folders, rail) = after(&ordering, &folders, &known, |o, f, rail| {
            delete_folder(o, f, rail, "folder-1")
        });
        assert_eq!(
            shape(&rail),
            vec![
                ("x".into(), ids(&["x"])),
                ("a".into(), ids(&["a"])),
                ("b".into(), ids(&["b"])),
                ("folder-2".into(), ids(&["y"])),
            ]
        );
        assert_eq!(folders.folders.len(), 1);
        assert!(!ordering.sidebar().contains(&"folder-1".to_string()));
    }

    #[test]
    fn folders_open_close_and_take_names_and_colours() {
        let mut folders = folders(json!({ "folders": [{ "id": "folder-1", "servers": ["a"] }] }));
        folders.toggle_folder("folder-1");
        assert!(folders.folder("folder-1").unwrap().collapsed);
        // Closed is written, and so syncs.
        assert!(folders.to_json().contains("\"collapsed\":true"));
        folders.toggle_folder("folder-1");
        assert!(!folders.folder("folder-1").unwrap().collapsed);

        folders.rename_folder("folder-1", "Games");
        folders.recolour_folder("folder-1", Some("#E5484D"));
        let folder = folders.folder("folder-1").unwrap();
        assert_eq!(
            (folder.name.as_str(), folder.colour.as_deref()),
            ("Games", Some("#E5484D"))
        );
        folders.recolour_folder("folder-1", None);
        assert_eq!(folders.folder("folder-1").unwrap().colour, None);
        assert!(!folders.to_json().contains("colour"));

        let before = folders.clone();
        folders.toggle_folder("folder-gone");
        folders.rename_folder("folder-gone", "Gone");
        folders.recolour_folder("folder-gone", Some("#E5484D"));
        assert_eq!(folders, before);
    }

    #[test]
    fn new_folder_ids_are_prefixed_and_unique() {
        let (one, two) = (new_folder_id(), new_folder_id());
        assert!(one.starts_with("folder-") && one.len() == "folder-".len() + 26);
        assert_ne!(one, two);
    }

    #[test]
    fn colours_are_read_from_short_and_long_hex() {
        assert_eq!(parse_colour("#abc"), Some(rgb(0xaabbcc)));
        assert_eq!(parse_colour("#AABBCC"), Some(rgb(0xaabbcc)));
        assert_eq!(parse_colour(" #30a46c "), Some(rgb(0x30a46c)));
        for colour in ["red", "", "#", "#abcd", "#ggg", "aabbcc", "rgb(1, 2, 3)"] {
            assert_eq!(parse_colour(colour), None, "{colour}");
        }
        for (name, hex) in PALETTE {
            assert!(parse_colour(hex).is_some(), "{name}");
        }
    }
}
