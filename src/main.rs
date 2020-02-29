use clap;
use rusqlite;
#[macro_use(format_err)]
extern crate failure;
use dirs;

use failure::Error;
use path_abs::PathAbs;
use regex::Regex;
use rusqlite::{Connection, NO_PARAMS};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use itertools::join;

const MIGRATIONS: [&str; 3] = [
    "
        begin transaction;

        create table if not exists jump_location (id INTEGER PRIMARY KEY ASC, location STRING UNIQUE, rank INTEGER);
        create index if not exists location_index on jump_location(location);

        insert into migration_version(version) values (1);

        commit;
    ",
    "
        begin transaction;

        drop table if exists temp_jump_location;
        alter table jump_location rename to temp_jump_location;

        create table jump_location (id INTEGER PRIMARY KEY ASC, location STRING UNIQUE COLLATE NOCASE, rank INTEGER);
        create index if not exists location_index on jump_location(location);

        insert or ignore into jump_location
            select id, location, rank from temp_jump_location;

        update migration_version set version = 2 where id = 1;

        commit;
    ",
    "
        begin transaction;

        alter table jump_location add column lastAccess TEXT;

        update jump_location set lastAccess = strftime('%Y-%m-%d %H:%M:%f', 'now', 'localtime');

        update migration_version set version = 3 where id = 1;

        commit;
    ",
];

struct Database {
    connection: Connection,
}

fn get_database_path() -> Result<PathBuf, Error> {
    if let Some(mut home) = dirs::home_dir() {
        home.push(".jumpjump");
        Ok(home)
    } else {
        Err(format_err!("Could not find home_dir to store jumpjump db"))
    }
}

fn ensure_tables(dbc: &Connection) -> Result<(), Error> {
    migrate(dbc, MIGRATIONS.len())?;
    add_regexp_function(dbc)
}

fn migrate(dbc: &Connection, desired_version: usize) -> Result<(), Error> {
    dbc.execute("create table if not exists migration_version (id INTEGER PRIMARY KEY ASC, version INTEGER);", NO_PARAMS)?;

    loop {
        let migration_version: usize = {
            let mut stmt =
                dbc.prepare("select version from migration_version where id = 1 limit 1")?;
            let mut results_iter = stmt.query_map(NO_PARAMS, |row| row.get::<_, u32>(0))?;
            match results_iter.next() {
                None => 0,
                Some(Ok(version)) => version as usize,
                Some(Err(err)) => {
                    return Err(format_err!("Failed to get database version: {}", err))
                }
            }
        };

        if migration_version == desired_version {
            return Ok(());
        }

        if migration_version > MIGRATIONS.len() {
            return Err(format_err!(
                "Unrecognized database version {}",
                migration_version
            ));
        }

        dbc.execute_batch(MIGRATIONS[migration_version as usize])?;
    }
}

fn add_regexp_function(db: &Connection) -> Result<(), Error> {
    let mut cached_regexes = HashMap::new();
    db.create_scalar_function("regexp", 2, true, move |ctx| {
        let regex_s = ctx.get::<String>(0)?;
        let entry = cached_regexes.entry(regex_s.clone());
        let regex = {
            use std::collections::hash_map::Entry::{Occupied, Vacant};
            match entry {
                Occupied(occ) => occ.into_mut(),
                Vacant(vac) => match Regex::new(&regex_s) {
                    Ok(r) => vac.insert(r),
                    Err(err) => return Err(rusqlite::Error::UserFunctionError(Box::new(err))),
                },
            }
        };

        let text = ctx.get::<String>(1)?;
        Ok(regex.is_match(&text))
    })?;

    Ok(())
}

fn canonicalize_path<P: AsRef<Path>>(path: P) -> Result<String, Error> {
    let canonical = PathAbs::new(path.as_ref())?;
    Ok(canonical.as_path().to_string_lossy().to_string())
}

impl Database {
    pub fn new(connection: Connection) -> Result<Database, Error> {
        ensure_tables(&connection)?;
        add_regexp_function(&connection)?;
        Ok(Database { connection })
    }

    pub fn add_location<S: AsRef<str>>(&self, location: S) -> Result<(), Error> {
        self.connection.execute(
            "insert into jump_location(location, rank, lastAccess) values(?, 1, strftime('%Y-%m-%d %H:%M:%f', 'now', 'localtime')) \
             on conflict(location) do update set rank=rank+1, lastAccess=strftime('%Y-%m-%d %H:%M:%f', 'now', 'localtime')",
            &[&location.as_ref()]
        )?;
        Ok(())
    }

    pub fn get_locations(&self) -> Result<Vec<String>, Error> {
        let mut stmt = self
            .connection
            .prepare("select location from jump_location order by rank desc, lastAccess desc")?;
        let locations = stmt
            .query_map(NO_PARAMS, |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(locations)
    }

    pub fn get_matching_locations<I>(&self, patterns: I) -> Result<Vec<String>, Error>
    where
        I: IntoIterator,
        I::Item: std::fmt::Display,
    {
        let pattern = format!("(?i).*{}.*", join(patterns, ".*"));
        let mut stmt = self.connection.prepare_cached(
            "select location from jump_location where regexp(?, location) order by rank desc, lastAccess desc",
        )?;
        let locations = stmt
            .query_map(&[&pattern], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(locations)
    }

    pub fn get_all_locations(&self) -> Result<Vec<String>, Error> {
        let mut stmt = self
            .connection
            .prepare("select location, rank, lastAccess from jump_location order by rank desc, lastAccess desc")?;
        let locations = stmt
            .query_map(NO_PARAMS, |row| {
                Ok(format!(
                    "{} {} {}",
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, String>(2)?
                ))
            })?
            .collect::<Result<Vec<String>, _>>()?;

        Ok(locations)
    }
}

fn report_locations(db: &Database) -> Result<(), Error> {
    let locations = db.get_locations()?;
    for l in locations.iter() {
        println!("{}", l);
    }
    Ok(())
}

fn report_all_locations(db: &Database) -> Result<(), Error> {
    let locations = db.get_all_locations()?;
    for l in locations.iter() {
        println!("{}", l);
    }
    Ok(())
}

fn report_best_location<I>(db: &Database, patterns: I) -> Result<(), Error>
where
    I: IntoIterator,
    I::Item: std::fmt::Display,
{
    let locations = db.get_matching_locations(patterns)?;
    if let Some(location) = locations.first() {
        println!("{}", location);
    }
    Ok(())
}

fn add_path<P: AsRef<Path>>(db: &Database, path: P) -> Result<(), Error> {
    let abs_path = canonicalize_path(path.as_ref())?;
    db.add_location(abs_path)?;
    Ok(())
}

fn main() -> Result<(), Error> {
    let matches = clap::App::new("jumpjump")
        .version("0.1")
        .author("Leaf Garland")
        .about("Jump around! Jump around! Jump up, jump up and get down!")
        .arg(
            clap::Arg::with_name("file")
                .short("f")
                .help("Use given db file instead of default"),
        )
        .subcommand(
            clap::SubCommand::with_name("add")
                .about("add location to db")
                .arg(clap::Arg::with_name("location").required(true).index(1)),
        )
        .subcommand(
            clap::SubCommand::with_name("get")
                .about("get recent location from db")
                .arg(clap::Arg::with_name("pattern").multiple(true).index(1)),
        )
        .subcommand(
            clap::SubCommand::with_name("show")
                .about("show all db entries"),
        )
        .get_matches();

    let default_path = get_database_path()?;
    let default_path_str = default_path.to_string_lossy();
    let db_path = matches.value_of("file").unwrap_or(&default_path_str);
    let connection = Connection::open(db_path)?;
    let db = Database::new(connection)?;

    match matches.subcommand() {
        ("add", Some(matches)) => {
            let location = matches.value_of("location").unwrap();
            add_path(&db, location)?;
        },
        ("get", Some(matches)) => {
            if let Some(patterns) = matches.values_of_lossy("pattern") {
                report_best_location(&db, patterns)?;
            } else {
                report_locations(&db)?;
            }
        },
        ("show", _) => {
            report_all_locations(&db)?;
        },
        _ => (),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_visited_location_is_first() {
        let db = Database::new(Connection::open_in_memory().unwrap()).unwrap();

        db.add_location("foo").unwrap();
        db.add_location("foo").unwrap();
        db.add_location("bar").unwrap();
        db.add_location("bar").unwrap();
        db.add_location("bar").unwrap();

        let locations: Vec<String> = db.get_locations().unwrap();

        assert_eq!(locations[..], ["bar", "foo"]);
    }

    #[test]
    fn finds_by_multiple_substr() {
        let db = Database::new(Connection::open_in_memory().unwrap()).unwrap();

        db.add_location("/foo/bar").unwrap();
        db.add_location("/foo/doo").unwrap();
        db.add_location("/foo/bar/doo").unwrap();

        let locations: Vec<String> = db.get_matching_locations(&["bar", "doo"]).unwrap();

        assert_eq!(locations[..], ["/foo/bar/doo"]);
    }

    #[test]
    fn finds_by_single_substr() {
        let db = Database::new(Connection::open_in_memory().unwrap()).unwrap();

        db.add_location("/foo/bar").unwrap();
        db.add_location("/foo/bar/doo").unwrap();

        let locations: Vec<String> = db.get_matching_locations(&["doo"]).unwrap();

        assert_eq!(locations[..], ["/foo/bar/doo"]);
    }

    #[test]
    fn finds_in_many() {
        let db = Database::new(Connection::open_in_memory().unwrap()).unwrap();

        for x in 0..10000 {
            db.add_location(format!("/foo/bar/{}", x)).unwrap();
        }

        let locations: Vec<String> = db.get_matching_locations(&["bar", "9999"]).unwrap();

        assert_eq!(locations[..], ["/foo/bar/9999"]);
    }
}
