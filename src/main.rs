extern crate byteorder;
extern crate clap;
extern crate reqwest;
extern crate roxmltree;
extern crate rusqlite;
extern crate tempfile;
extern crate zip;

mod player;
mod podcast;

use player::Player;
use podcast::Podcast;

use std::borrow::{Borrow, BorrowMut};
use std::collections::HashMap;
use std::error;
use std::fmt;
use std::io;
use std::path::Path;

use clap::{Arg, ArgGroup, ArgMatches};
use rusqlite::Connection;
use tempfile::NamedTempFile;

pub type BoxResult<T> = Result<T, Box<dyn error::Error>>;

#[derive(Debug, Clone)]
pub enum Error {
	InvalidArguments(usize),
	InvalidUUID,
	MissingXMLNode(&'static str),
	InvalidPlayingStatus,
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Error::InvalidArguments(n) => write!(f, "Invalid number of call arguments: {}", n),
			Error::InvalidUUID => write!(f, "String is not a valid UUID"),
			Error::MissingXMLNode(node) => write!(f, "Missing XML node: {}", node),
			Error::InvalidPlayingStatus => write!(f, "Invalid playing status"),
		}
	}
}

impl error::Error for Error {
	fn source(&self) -> Option<&(dyn error::Error + 'static)> {
		None
	}
}

#[derive(PartialEq)]
struct UUID(u128);

impl fmt::Debug for UUID {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "FeedId({})", self.to_string())
	}
}

impl ToString for UUID {
	fn to_string(&self) -> String {
		format!(
			"{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
			(self.0 >> 96) as u32,
			(self.0 >> 80) as u16,
			(self.0 >> 64) as u16,
			(self.0 >> 48) as u16,
			(self.0 as u64) & 0xFFFFFFFFFFFF,
		)
	}
}

impl UUID {
	fn from_str(s: String) -> BoxResult<Self> {
		if s.len() != 36
			|| &s[8..9] != "-"
			|| &s[13..14] != "-"
			|| &s[18..19] != "-"
			|| &s[23..24] != "-"
		{
			return Err(Error::InvalidUUID.into());
		}

		let s_num = s[..8].to_string() + &s[9..13] + &s[14..18] + &s[19..23] + &s[24..];
		u128::from_str_radix(&s_num, 16)
			.map(|n| Self(n))
			.map_err(|err| err.into())
	}
}

struct SQLLiteDatabase {
	conn: Connection,
	file: NamedTempFile,
}

impl Borrow<Connection> for SQLLiteDatabase {
	fn borrow(&self) -> &Connection {
		&self.conn
	}
}

impl BorrowMut<Connection> for SQLLiteDatabase {
	fn borrow_mut(&mut self) -> &mut Connection {
		&mut self.conn
	}
}

impl SQLLiteDatabase {
	pub fn open<P: AsRef<Path>>(path: P) -> BoxResult<Self> {
		SQLLiteDatabase::open_from_reader(&mut std::fs::File::open(path)?)
	}

	pub fn open_from_reader<R: io::Read>(r: &mut R) -> BoxResult<Self> {
		let mut temp_file = NamedTempFile::new()?;
		io::copy(r, &mut temp_file)?;

		Ok(Self {
			conn: Connection::open(temp_file.path())?,
			file: temp_file,
		})
	}

	pub fn into_file(self) -> BoxResult<std::fs::File> {
		if let Err((_, err)) = self.conn.close() {
			return Err(Box::new(err));
		}
		self.file.reopen().map_err(|err| err.into())
	}
}

struct PlayerArgs {
	cli_name: &'static str,
	in_name: String,
	out_name: String,
	player_help: String,
	in_help: String,
	out_help: String,
	factory: fn(&str) -> BoxResult<Box<dyn Player>>,
}

impl PlayerArgs {
	fn new<T: player::NewPlayer>() -> Self {
		Self {
			cli_name: T::cli_name(),
			in_name: String::from("in-") + T::cli_name(),
			out_name: String::from("out-") + T::cli_name(),
			player_help: String::from("the ") + T::name() + " save file",
			in_help: String::from("Convert from ") + T::name(),
			out_help: String::from("Convert to ") + T::name() + " and output to FILE",
			factory: T::new,
		}
	}

	fn get(&self) -> [Arg<'_, '_>; 3] {
		let player_arg = Arg::with_name(self.cli_name)
			.long(self.cli_name)
			.takes_value(true)
			.value_name("FILE")
			.help(self.player_help.as_str());

		let in_arg = Arg::with_name(self.in_name.as_str())
			.long(self.in_name.as_str())
			.requires(self.cli_name)
			.group("in")
			.help(self.in_help.as_str());

		let out_arg = Arg::with_name(self.out_name.as_str())
			.long(self.out_name.as_str())
			.requires(self.cli_name)
			.group("out")
			.takes_value(true)
			.value_name("FILE")
			.help(self.out_help.as_str());

		[player_arg, in_arg, out_arg]
	}

	fn create_player(&self, matches: &ArgMatches) -> Option<BoxResult<Box<dyn Player>>> {
		matches
			.value_of(self.cli_name)
			.map(|path| (self.factory)(path))
	}
}

fn populate(player: &mut Box<dyn Player>, podcasts: Vec<Podcast>) -> BoxResult<Vec<Podcast>> {
	podcasts
		.into_iter()
		.map(|pod| {
			println!("Populating '{}' ({})", pod.title, pod.url);
			player.populate(pod)
		})
		.collect()
}

fn get_players(
	matches: &ArgMatches,
	players_args: &[PlayerArgs],
) -> BoxResult<HashMap<&'static str, Box<dyn Player>>> {
	let kv_pairs = players_args
		.iter()
		.filter_map(|player_args| {
			player_args
				.create_player(matches)
				.map(|res| res.map(|player| (player_args.cli_name, player)))
		})
		.collect::<BoxResult<Vec<(&'static str, Box<dyn Player>)>>>()?;
	Ok(kv_pairs.into_iter().collect())
}

fn main() -> BoxResult<()> {
	// Array of posible players
	let players_args = [
		PlayerArgs::new::<player::BeyondPod>(),
		PlayerArgs::new::<player::PocketCasts>(),
	];

	// Construct global cli
	let mut app = clap::App::new("podcast_history_converter")
		.arg(
			Arg::with_name("opml")
				.long("opml")
				.takes_value(true)
				.value_name("FILE")
				.help("OPML file containing all the feeds to convert")
				.required(true),
		)
		.group(ArgGroup::with_name("in").required(true))
		.group(ArgGroup::with_name("out").required(true).multiple(true));

	// Add cli for each player
	for player_args in players_args.iter() {
		app = app.args(&player_args.get());
	}

	// Parse cli args
	let matches = app.get_matches();

	// Initialise all the players needed for the given args
	let mut players = get_players(&matches, &players_args)?;

	// Get cli name of the source player
	let in_player = players_args
		.iter()
		.find(|player_args| matches.is_present(player_args.in_name.as_str()))
		.map(|player_args| player_args.cli_name)
		.expect("input player not found in args list");

	// Get (cli name of destination player, output file path) pairs for the given args
	let outputs: Vec<(&'static str, &'_ str)> = players_args
		.iter()
		.filter_map(|player_args| {
			matches
				.value_of(player_args.out_name.as_str())
				.map(|path| (player_args.cli_name, path))
		})
		.collect();

	// Parse the given OPML file and pull podcast data
	let podcasts = podcast::from_opml(matches.value_of("opml").expect("no opml file"))?;

	// Populate empty track data from the source player
	let podcasts = populate(
		players.get_mut(in_player).expect("input player not found"),
		podcasts,
	)?;

	// Loop through the output pairs
	for (player, path) in outputs.into_iter() {
		println!("Saving to '{}'", player);

		// Remove player from map
		let p = players.remove(player).expect("input player not found");

		// Open output file
		let mut out_file = std::fs::OpenOptions::new()
			.write(true)
			.create(true)
			.truncate(true)
			.open(path)?;

		// Write podcast data to the output file
		p.save(&mut podcasts.iter(), &mut out_file)?;
		out_file.sync_all()?;
	}

	Ok(())
}
