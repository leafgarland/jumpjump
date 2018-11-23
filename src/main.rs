extern crate rusqlite;
#[macro_use(format_err)]
extern crate failure;
extern crate dirs;
extern crate itertools;
extern crate path_abs;
extern crate regex;
extern crate url;

use failure::Error;
use path_abs::PathArc;
use regex::Regex;
use rusqlite::{Connection, NO_PARAMS};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use url::Url;

use itertools::join;

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
	dbc.execute("create table if not exists jump_location (id INTEGER PRIMARY KEY ASC, location STRING UNIQUE, rank INTEGER);
				 create index if not exists location_index on jump_location(location)", NO_PARAMS)?;
	add_regexp_function(dbc)?;
	Ok(())
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
	let canonical = PathArc::new(path.as_ref()).absolute()?;
	if cfg!(target_os = "windows") {
		let url =
			Url::from_file_path(&canonical).map_err(|_| format_err!("Failed to build url"))?;
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
	pub fn new(connection: Connection) -> Result<Database, Error> {
		ensure_tables(&connection)?;
		add_regexp_function(&connection)?;
		Ok(Database { connection })
	}

	pub fn add_location<S: AsRef<str>>(&self, location: S) -> Result<(), Error> {
		self.connection.execute(
			"insert into jump_location(location, rank) values(?, 1) on conflict(location) do update set rank=rank+1",
			&[&location.as_ref()]
		)?;
		Ok(())
	}

	pub fn get_locations(&self) -> Result<Vec<String>, Error> {
		let mut stmt = self
			.connection
			.prepare("select location from jump_location order by rank desc")?;
		let results_iter = stmt.query_map(NO_PARAMS, |row| row.get(0))?;

		let mut locations = Vec::new();
		for r in results_iter {
			locations.push(r?);
		}

		Ok(locations)
	}

	pub fn get_matching_locations(&self, patterns: &[&str]) -> Result<Vec<String>, Error> {
		let pattern = format!("(?i).*{}.*", join(patterns, ".*"));
		let mut stmt = self.connection.prepare_cached(
			"select location from jump_location where regexp(?, location) order by rank desc",
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
		println!("{}", l);
	}
	Ok(())
}

fn report_best_location(db: &Database, patterns: &[&str]) -> Result<(), Error> {
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
	let args: Vec<String> = env::args().collect();
	let db_path = get_database_path()?;
	let connection = Connection::open(db_path)?;
	let db = Database::new(connection)?;

	match args.iter().map(|s| s.as_ref()).collect::<Vec<&str>>()[1..] {
		["add", location] => add_path(&db, location)?,
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
