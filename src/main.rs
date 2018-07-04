extern crate rusqlite;
#[macro_use(format_err)]
extern crate failure;
extern crate url;

use failure::Error;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use url::Url;

struct Database {
    connection: Connection,
}

fn get_database_path() -> Result<PathBuf, Error> {
    if let Some(mut home) = std::env::home_dir() {
        home.push(".jumpjump");
        Ok(home)
    } else {
        Err(format_err!("Could not find home_dir to store jumpjump db"))
    }
}

fn ensure_tables(dbc: &Connection) -> Result<(), Error> {
    dbc.execute("create table if not exists jump_location (id INTEGER PRIMARY KEY ASC, location STRING UNIQUE)", &[])?;
    Ok(())
}

fn canonicalize_path<P: AsRef<Path>>(path: P) -> Result<PathBuf, Error> {
    let canonical = path.as_ref().canonicalize()?;
    if cfg!(target_os = "windows") {
        let url = Url::from_file_path(&canonical).map_err(|_| format_err!("Failed to build url"))?;
        let path = url.to_file_path()
            .map_err(|_| format_err!("Failed to build url"))?;
        return Ok(path);
    }
    Ok(canonical)
}

impl Database {
    pub fn new() -> Result<Database, Error> {
        let connection = Connection::open(get_database_path()?)?;
        ensure_tables(&connection)?;
        Ok(Database { connection })
    }

    pub fn clear(&self) -> Result<(), Error> {
        self.connection.execute("delete from jump_location", &[])?;
        Ok(())
    }

    pub fn add_location<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        let a_path = canonicalize_path(path.as_ref())?;
        self.connection.execute(
            "insert or ignore into jump_location(location) values(?)",
            &[&a_path.to_str().unwrap()],
        )?;
        Ok(())
    }
}

fn main() -> Result<(), Error> {
    let db = Database::new()?;

    db.clear()?;
    db.add_location("/dev/tools")?;

    Ok(())
}

// fn foo() {
//     let mut stmt = conn.prepare("SELECT id, name, time_created, data FROM person")?;
//     let person_iter = stmt.query_map(&[], |row| {
//         Person {
//             id: row.get(0),
//             name: row.get(1),
//             time_created: row.get(2),
//             data: row.get(3)
//         }
//     })?;

//     for person in person_iter {
//         println!("Found person {:?}", person?);
//     }

//     Ok(())
// }
