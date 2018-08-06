extern crate rusqlite;
#[macro_use(format_err)]
extern crate failure;
extern crate itertools;
extern crate url;
extern crate path_abs;

use failure::Error;
use rusqlite::{version, Connection};
use std::env;
use std::path::{Path, PathBuf};
use path_abs::PathArc;
use url::Url;

use itertools::join;

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
    // let canonical = path.as_ref().canonicalize()?;
    let canonical = PathArc::new(path.as_ref()).absolute()?;
    if cfg!(target_os = "windows") {
        let url = Url::from_file_path(&canonical).map_err(|_| format_err!("Failed to build url"))?;
        let path = url
            .to_file_path()
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

    // pub fn clear(&self) -> Result<(), Error> {
    //     self.connection.execute("delete from jump_location", &[])?;
    //     Ok(())
    // }

    pub fn add_location<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        let a_path = canonicalize_path(path.as_ref())?;
        self.connection.execute(
            // use this with sqlite >= 3.24
            "insert into jump_location(location, rank) values(?, 1) on conflict(location) do update set rank=rank+1",
            // use this with sqlite < 3.24
            // "with new(location) as (values(?)) insert or replace into jump_location(id, location, rank)
            //  select old.id, new.location, (coalesce(old.rank + 1, 1)) as rank from new left join jump_location old on old.location = new.location",
            &[&a_path]
        )?;
        Ok(())
    }

    pub fn get_locations(&self) -> Result<Vec<String>, Error> {
        let mut stmt = self
            .connection
            .prepare("select location from jump_location order by rank desc")?;
        let results_iter = stmt.query_map(&[], |row| row.get(0))?;

        let mut locations = Vec::new();
        for r in results_iter {
            locations.push(r?);
        }

        Ok(locations)
    }

    pub fn get_matching_locations(&self, patterns: &[&str]) -> Result<Vec<String>, Error> {
        let mut pattern = join(patterns, "*");
        pattern.insert(0, '*');
        pattern.push('*');
        let mut stmt = self.connection.prepare_cached(
            "select location from jump_location where location glob ? order by rank desc",
        )?;
        let results_iter = stmt.query_map(&[&pattern], |row| row.get(0))?;

        let mut locations = Vec::new();
        for r in results_iter {
            locations.push(r?);
        }

        Ok(locations)
    }
}

fn report_all_locations(db: &Database) -> Result<(), Error> {
    let locations = db.get_locations()?;
    for l in locations.iter() {
        println!("loc: {:?}", l);
    }
    Ok(())
}

fn report_best_location(db: &Database, patterns: &[&str]) -> Result<(), Error> {
    let locations = db.get_matching_locations(patterns)?;
    if let Some(location) = locations.first() {
        println!("loc: {:?}", location);
    }
    Ok(())
}

fn main() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();

    let db = Database::new(Option::Some(get_database_path()?))?;
    match args.iter().map(|s| s.as_ref()).collect::<Vec<&str>>()[1..] {
        ["add", location] => db.add_location(location)?,
        ["get"] => report_all_locations(&db)?,
        ref largs if largs[0] == "get" => report_best_location(&db, &largs[1..])?,
        _ => println!("Failed to parse arguments"),
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foo() {
        let db = Database::new(Option::None).unwrap();

        db.add_location("/dev/tools").unwrap();
        db.add_location("/dev/tools").unwrap();
        db.add_location("/dev/tools/vim").unwrap();
        db.add_location("/dev/tools/vim").unwrap();
        db.add_location("/dev/tools/vim").unwrap();

        let locations: Vec<String> = db.get_locations().unwrap();

        assert_eq!(locations[..], ["C:/dev/tools/vim", "C:/dev/tools"]);
    }

    #[test]
    fn get() {
        let db = Database::new(Option::None).unwrap();

        db.add_location("/dev/tools").unwrap();
        db.add_location("/dev/tools/vim").unwrap();

        let locations: Vec<String> = db.get_matching_locations(&["vim"]).unwrap();

        assert_eq!(locations[..], ["C:/dev/tools/vim"]);
    }

    #[test]
    fn non_existing_path() {
        let db = Database::new(Option::None).unwrap();

        db.add_location("foo/bar/doolally").unwrap();

        let locations: Vec<String> = db.get_locations().unwrap();

        assert_eq!(locations[..], ["C:/dev/me/jumpjump/foo/bar/doolally"]);
    }
}
