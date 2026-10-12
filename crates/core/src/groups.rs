//! User-defined session groups for the Sessions board. Stored in Peekaboo's own data folder;
//! Claude Code's transcripts are never modified.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GroupData {
    pub groups: Vec<Group>,
    /// session id → group id
    pub assignments: BTreeMap<String, String>,
    next_id: u64,
}

pub struct GroupStore {
    path: PathBuf,
    data: GroupData,
}

impl GroupStore {
    /// Missing file → empty. Corrupt file → renamed to `.bad`, empty.
    pub fn load(path: &Path) -> GroupStore {
        let data = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                eprintln!("peekaboo: corrupt groups file {}: {e}", path.display());
                let _ = std::fs::rename(path, path.with_extension("json.bad"));
                GroupData::default()
            }),
            Err(_) => GroupData::default(),
        };
        GroupStore { path: path.to_path_buf(), data }
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.data)?)?;
        std::fs::rename(tmp, &self.path)
    }

    pub fn data(&self) -> &GroupData {
        &self.data
    }

    pub fn groups(&self) -> &[Group] {
        &self.data.groups
    }

    pub fn group_of(&self, session: &str) -> Option<&str> {
        self.data.assignments.get(session).map(String::as_str)
    }

    fn find_by_name(&self, name: &str) -> Option<&Group> {
        self.data.groups.iter().find(|g| g.name.eq_ignore_ascii_case(name))
    }

    fn valid_name(&self, name: &str, except: Option<&str>) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("group name can't be empty".into());
        }
        if self.find_by_name(name).is_some_and(|g| Some(g.id.as_str()) != except) {
            return Err(format!("a group called \"{name}\" already exists"));
        }
        Ok(name.chars().take(60).collect())
    }

    pub fn create(&mut self, name: &str) -> Result<Group, String> {
        let name = self.valid_name(name, None)?;
        self.data.next_id += 1;
        let g = Group { id: format!("g{}", self.data.next_id), name };
        self.data.groups.push(g.clone());
        Ok(g)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = self.valid_name(name, Some(id))?;
        let g = self.data.groups.iter_mut().find(|g| g.id == id).ok_or("no such group")?;
        g.name = name;
        Ok(())
    }

    /// Removes the group; its sessions become ungrouped.
    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let before = self.data.groups.len();
        self.data.groups.retain(|g| g.id != id);
        if self.data.groups.len() == before {
            return Err("no such group".into());
        }
        self.data.assignments.retain(|_, g| g != id);
        Ok(())
    }

    pub fn assign(&mut self, session: &str, group: Option<&str>) -> Result<(), String> {
        match group {
            None => {
                self.data.assignments.remove(session);
            }
            Some(id) => {
                if !self.data.groups.iter().any(|g| g.id == id) {
                    return Err("no such group".into());
                }
                self.data.assignments.insert(session.to_string(), id.to_string());
            }
        }
        Ok(())
    }

    /// Puts every ungrouped session into a group named after its project, creating groups as
    /// needed. Sessions the user already grouped are left alone. Returns how many were moved.
    pub fn auto_group(&mut self, sessions: &[(&str, &str)]) -> usize {
        let mut moved = 0;
        for (session, project) in sessions {
            if self.group_of(session).is_some() || project.is_empty() || *project == "unknown" {
                continue;
            }
            let id = match self.find_by_name(project) {
                Some(g) => g.id.clone(),
                None => match self.create(project) {
                    Ok(g) => g.id,
                    Err(_) => continue,
                },
            };
            self.data.assignments.insert(session.to_string(), id);
            moved += 1;
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, GroupStore) {
        let d = tempfile::tempdir().unwrap();
        let s = GroupStore::load(&d.path().join("groups.json"));
        (d, s)
    }

    #[test]
    fn create_rename_assign_persist() {
        let (d, mut s) = store();
        let g = s.create("Foxtale").unwrap();
        assert!(s.create("  ").is_err());
        assert!(s.create("foxtale").is_err(), "names are unique, case-insensitively");
        s.assign("sess-1", Some(&g.id)).unwrap();
        s.rename(&g.id, "Foxtale CMS").unwrap();
        s.save().unwrap();
        let back = GroupStore::load(&d.path().join("groups.json"));
        assert_eq!(back.groups()[0].name, "Foxtale CMS");
        assert_eq!(back.group_of("sess-1"), Some(g.id.as_str()));
    }

    #[test]
    fn unassign_and_unknown_group() {
        let (_d, mut s) = store();
        let g = s.create("A").unwrap();
        s.assign("x", Some(&g.id)).unwrap();
        s.assign("x", None).unwrap();
        assert_eq!(s.group_of("x"), None);
        assert!(s.assign("x", Some("nope")).is_err());
    }

    #[test]
    fn deleting_a_group_ungroups_its_sessions() {
        let (_d, mut s) = store();
        let a = s.create("A").unwrap();
        let b = s.create("B").unwrap();
        s.assign("1", Some(&a.id)).unwrap();
        s.assign("2", Some(&b.id)).unwrap();
        s.delete(&a.id).unwrap();
        assert_eq!(s.groups().iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["B"]);
        assert_eq!(s.group_of("1"), None);
        assert_eq!(s.group_of("2"), Some(b.id.as_str()));
    }

    #[test]
    fn auto_group_by_project_only_touches_ungrouped_and_reuses_names() {
        let (_d, mut s) = store();
        let mine = s.create("Mine").unwrap();
        let shop = s.create("shop").unwrap();
        s.assign("manual", Some(&mine.id)).unwrap();
        let moved = s.auto_group(&[("manual", "shop"), ("a", "shop"), ("b", "site"), ("c", "site"), ("d", "unknown")]);
        assert_eq!(moved, 3);
        assert_eq!(s.group_of("manual"), Some(mine.id.as_str()), "manual choice kept");
        assert_eq!(s.group_of("a"), Some(shop.id.as_str()), "existing group reused");
        let site = s.groups().iter().find(|g| g.name == "site").unwrap().id.clone();
        assert_eq!(s.group_of("b"), Some(site.as_str()));
        assert_eq!(s.group_of("d"), None, "sessions without a project stay ungrouped");
    }

    #[test]
    fn corrupt_file_starts_empty_and_is_kept_aside() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("groups.json");
        std::fs::write(&p, "{oops").unwrap();
        let s = GroupStore::load(&p);
        assert!(s.groups().is_empty());
        assert!(d.path().join("groups.json.bad").exists());
    }
}
