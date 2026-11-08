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

use std::collections::{HashMap, HashSet};

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
}
