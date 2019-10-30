use crate::player::{IoWriteSeek, NewPlayer, Player};
use crate::podcast::{PlayingStatus, Podcast};
use crate::{BoxResult, SQLLiteDatabase, UUID};

use std::borrow::Borrow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Seek};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use reqwest::Url;
use rusqlite::Connection;

const HISTORY_FILE: &str = "BeyondPodItemHistory.bin.autobak";
const DB_FILE: &str = "beyondpod.db.autobak";

struct HistoryTokenIter<R: ReadBytesExt> {
	r: R,
}

impl<R: ReadBytesExt> HistoryTokenIter<R> {
	fn new(r: R) -> Self {
		Self { r: r }
	}
}

impl<R: ReadBytesExt> Iterator for HistoryTokenIter<R> {
	type Item = (String, u32);

	fn next(&mut self) -> Option<Self::Item> {
		// Read string length
		let str_len = self.r.read_u16::<BigEndian>().ok()?;

		// Read string
		let mut buf = vec![0u8; str_len as usize];
		self.r.read_exact(&mut buf).ok()?;
		let string = String::from_utf8(buf).ok()?;

		// Read data word
		let data = self.r.read_u32::<BigEndian>().ok()?;

		Some((string, data))
	}
}

pub struct BeyondPod {
	archive: zip::ZipArchive<File>,
	db: SQLLiteDatabase,
}

impl BeyondPod {
	fn guid_to_track_id(guid: &String) -> u32 {
		let mut acc = 0u32;
		for &b in guid.as_bytes() {
			acc = acc.wrapping_mul(31).wrapping_add(b as u32);
		}
		acc
	}

	fn get_feed(&self, url: &Url) -> BoxResult<(UUID, i32)> {
		let conn: &Connection = self.db.borrow();
		let mut stmt = conn.prepare("SELECT feedid, hasunread FROM feeds WHERE url = :url")?;
		let mut rows = stmt.query_named(&[(":url", &url.to_string())])?;
		let first_row = rows.next()?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
		Ok((UUID::from_str(first_row.get(0)?)?, first_row.get(1)?))
	}

	fn get_track(&self, feed_id: &UUID, track_id: u32) -> rusqlite::Result<(bool, i32)> {
		let conn: &Connection = self.db.borrow();
		let mut stmt = conn
			.prepare("SELECT played,playedtime FROM tracks WHERE orgrssitemid = :orgrssitemid and parentfeedid = :parentfeedid")?;
		let mut rows = stmt.query_named(&[
			(":orgrssitemid", &(track_id as i32).to_string()),
			(":parentfeedid", &feed_id.to_string()),
		])?;
		let first_row = rows.next()?;
		first_row
			.ok_or(rusqlite::Error::QueryReturnedNoRows)
			.and_then(|row| Ok((row.get(0)?, row.get(1)?)))
	}

	fn update_track(
		&self,
		feed_id: &UUID,
		track_id: u32,
		played: bool,
		played_time: i32,
	) -> rusqlite::Result<()> {
		let conn: &Connection = self.db.borrow();
		conn.execute_named(
			"UPDATE tracks SET played = :played, playedtime = :playedtime WHERE orgrssitemid = :orgrssitemid and parentfeedid = :parentfeedid",
			&[
				(":orgrssitemid", &(track_id as i32).to_string()),
				(":parentfeedid", &feed_id.to_string()),
				(":played", &played),
				(":playedtime", &played_time),
			],
		).map(|_| ())
	}

	fn get_feed_history(&mut self, feed: &UUID) -> BoxResult<HashMap<u32, u32>> {
		let item_history = self.archive.by_name(HISTORY_FILE)?;
		let mut iter = HistoryTokenIter::new(item_history);

		while let Some((id_str, count)) = iter.next() {
			let id = UUID::from_str(id_str)?;
			if &id == feed {
				return iter
					.take(count as usize)
					.map(|(track_str, flags)| {
						Ok((i32::from_str_radix(track_str.as_str(), 10)? as u32, flags))
					})
					.collect();
			}

			for _ in 0..count {
				if let None = iter.next() {
					break;
				}
			}
		}

		Ok(HashMap::new())
	}

	fn write_history_token<W: WriteBytesExt>(
		w: &mut W,
		string: String,
		data: u32,
	) -> io::Result<()> {
		w.write_u16::<BigEndian>(string.len() as u16)?;
		w.write(string.as_bytes())?;
		w.write_u32::<BigEndian>(data)
	}

	fn write_feed_history<W: WriteBytesExt>(
		w: &mut W,
		id: &UUID,
		feed: Vec<(u32, u32)>,
	) -> io::Result<()> {
		Self::write_history_token(w, id.to_string(), feed.len() as u32)?;
		for (id, flags) in feed.into_iter() {
			Self::write_history_token(w, (id as i32).to_string(), flags)?;
		}
		Ok(())
	}
}

impl Player for BeyondPod {
	fn populate(&mut self, mut podcast: Podcast) -> BoxResult<Podcast> {
		let (id, _unread) = self.get_feed(&podcast.url)?;
		let history = self.get_feed_history(&id)?;

		for track in podcast.tracks.iter_mut() {
			let track_id = BeyondPod::guid_to_track_id(&track.guid);

			let (sql_played, sql_progress) = self.get_track(&id, track_id).ok().map_or_else(
				|| (None, None),
				|(played, played_time)| {
					(
						Some(played),
						if played_time >= 0 {
							Some(played_time)
						} else {
							None
						},
					)
				},
			);

			let history_played = history.get(&track_id).map(|&flags| flags == 65);

			let played =
				if let Some((sql, history)) = sql_played.and_then(|s| Some((s, history_played?))) {
					if sql == history {
						sql
					} else {
						println!(
							"{}: played history mismatch: sql={}, history={}",
							track.url, sql, history
						);
						false
					}
				} else {
					sql_played.xor(history_played).unwrap_or(false)
				};

			track.progress = sql_progress.unwrap_or(0);

			track.playing_status = if played {
				PlayingStatus::Played
			} else {
				if track.progress > 0 {
					PlayingStatus::Playing
				} else {
					PlayingStatus::Unplayed
				}
			}
		}

		Ok(podcast)
	}

	fn save(
		mut self: Box<Self>,
		podcasts: &mut dyn Iterator<Item = &'_ Podcast>,
		w: &mut dyn IoWriteSeek,
	) -> BoxResult<()> {
		let mut zip = zip::ZipWriter::new(w);
		let options = zip::write::FileOptions::default();

		let mut new_hist_file = io::Cursor::new(vec![0; 0]);

		for podcast in podcasts {
			println!("Saving '{}' ({})", podcast.title, podcast.url);
			let (id, _unread) = self.get_feed(&podcast.url)?;
			let mut history_tracks: Vec<(u32, u32)> = Vec::with_capacity(podcast.tracks.len());

			for track in podcast.tracks.iter() {
				let track_id = BeyondPod::guid_to_track_id(&track.guid);
				let played = track.playing_status == PlayingStatus::Played;
				let is_in_db = self.get_track(&id, track_id).is_ok();

				if is_in_db {
					self.update_track(&id, track_id, played, track.progress)?;
				}

				if played || is_in_db {
					history_tracks.push((track_id, if played { 65 } else { 64 }));
				}
			}

			if history_tracks.len() > 0 {
				Self::write_feed_history(&mut new_hist_file, &id, history_tracks)?;
			}
		}

		let mut db_temp_file = self.db.into_file()?;

		// Copy all the files from the input archive to the output archive
		for i in 0..self.archive.len() {
			let mut in_file = self.archive.by_index(i)?;
			let file_name = in_file.name().to_owned();

			let out_file: &mut dyn io::Read = match file_name.as_str() {
				HISTORY_FILE => {
					new_hist_file.seek(io::SeekFrom::Start(0))?;
					&mut new_hist_file
				}
				DB_FILE => &mut db_temp_file,
				_ => &mut in_file,
			};

			zip.start_file(file_name, options)?;
			io::copy(out_file, &mut zip)?;
		}

		zip.finish()?;
		Ok(())
	}
}

impl NewPlayer for BeyondPod {
	fn new(path: &str) -> BoxResult<Box<dyn Player>> {
		let f = File::open(path)?;
		let mut archive = zip::ZipArchive::new(f)?;
		let db = SQLLiteDatabase::open_from_reader(&mut archive.by_name(DB_FILE)?)?;

		Ok(Box::new(Self {
			archive: archive,
			db: db,
		}))
	}

	fn name() -> &'static str {
		"BeyondPod"
	}
	fn cli_name() -> &'static str {
		"beyondpod"
	}
}
