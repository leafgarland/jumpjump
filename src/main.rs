extern crate rusqlite;
#[macro_use(format_err)]
extern crate failure;
extern crate url;

use failure::Error;
use rusqlite::{Connection, version};
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
    dbc.execute("create table if not exists jump_location (id INTEGER PRIMARY KEY ASC, location STRING UNIQUE, rank INTEGER);
                 create index if not exists location_index on jump_location(location)", &[])?;
    Ok(())
}

fn canonicalize_path<P: AsRef<Path>>(path: P) -> Result<String, Error> {
    let canonical = path.as_ref().canonicalize()?;
    if cfg!(target_os = "windows") {
        let url = Url::from_file_path(&canonical).map_err(|_| format_err!("Failed to build url"))?;
        let path = url.to_file_path()
            .map_err(|_| format_err!("Failed to build url"))?;
        let cow = path.to_string_lossy();
        let fs = cow.replace('\\', "/");
        return Ok(fs);
    }
    Ok(canonical.to_string_lossy().to_string())
}

impl Database {
    pub fn new(path: Option<PathBuf>) -> Result<Database, Error> {
        let connection = match path {
            Some(path) => Connection::open(path)?,
            None => Connection::open_in_memory()?,
        };
        println!("db version is {}", version());
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
            // can use this with sqlite 3.24+
            // "insert into jump_location(location, rank) values(?, 1) on conflict(location) do update set rank=rank+1",
            "with new(location) as (values(?)) insert or replace into jump_location(id, location, rank)
             select old.id, new.location, (coalesce(old.rank + 1, 1)) as rank from new left join jump_location old on old.location = new.location",
            &[&a_path]
        )?;
        Ok(())
    }

    pub fn get_locations(&self) -> &[&str] {
        let mut stmt = self.connection.prepare("select * from jump_location").unwrap();
    }
}

fn main() -> Result<(), Error> {
    let db = Database::new(Option::None)?;

    db.clear()?;
    db.add_location("/dev/tools")?;
    db.add_location("/dev/tools")?;
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
