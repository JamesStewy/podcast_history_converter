use crate::player::{IoWriteSeek, NewPlayer, Player};
use crate::podcast::{PlayingStatus, Podcast};
use crate::{BoxResult, Error, SQLLiteDatabase, UUID};

use std::borrow::Borrow;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Url;
use rusqlite::Connection;

pub struct PocketCasts {
	db: SQLLiteDatabase,
}

impl PocketCasts {
	fn get_podcast(&self, title: &String) -> BoxResult<UUID> {
		let conn: &Connection = self.db.borrow();
		let mut stmt = conn.prepare("SELECT uuid FROM podcasts WHERE title = :title")?;
		let mut rows = stmt.query_named(&[(":title", title)])?;
		let first_row = rows.next()?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
		Ok(UUID::from_str(first_row.get(0)?)?)
	}

	fn get_episode(&self, podcast_id: &UUID, episode_url: &Url) -> rusqlite::Result<(i64, f64)> {
		let conn: &Connection = self.db.borrow();
		let mut stmt = conn
			.prepare("SELECT playing_status, played_up_to FROM episodes WHERE podcast_id = :podcast_id AND download_url = :download_url")?;
		let mut rows = stmt.query_named(&[
			(":podcast_id", &podcast_id.to_string()),
			(":download_url", &episode_url.to_string()),
		])?;
		let first_row = rows.next()?;
		first_row
			.ok_or(rusqlite::Error::QueryReturnedNoRows)
			.and_then(|row| Ok((row.get(0)?, row.get(1)?)))
	}

	fn update_episode_part(
		&self,
		podcast_id: &UUID,
		episode_url: &Url,
		field: &'static str,
		value: i32,
		time: i64,
	) -> rusqlite::Result<()> {
		let conn: &Connection = self.db.borrow();
		conn.execute_named(
			("UPDATE episodes SET ".to_string() + field + " = :value, " + field + "_modified = :time WHERE podcast_id = :podcast_id AND download_url = :download_url AND " + field + " <> :value").as_str(),
			&[
				(":podcast_id", &podcast_id.to_string()),
				(":download_url", &episode_url.to_string()),
				(":value", &value),
				(":time", &time),
			],
		).map(|_| ())
	}

	fn update_episode(
		&self,
		podcast_id: &UUID,
		episode_url: &Url,
		played_up_to: i32,
		playing_status: i32,
		time: i64,
	) -> rusqlite::Result<()> {
		self.update_episode_part(podcast_id, episode_url, "played_up_to", played_up_to, time)?;
		self.update_episode_part(
			podcast_id,
			episode_url,
			"playing_status",
			playing_status,
			time,
		)
	}
}

impl Player for PocketCasts {
	fn populate(&mut self, mut podcast: Podcast) -> BoxResult<Podcast> {
		let id = self.get_podcast(&podcast.title)?;

		for track in podcast.tracks.iter_mut() {
			match self.get_episode(&id, &track.url) {
				Ok((playing_status_i, played_up_to_f)) => {
					let played_up_to = played_up_to_f as i32;
					track.progress = std::cmp::max(played_up_to, 0);

					track.playing_status = match playing_status_i {
						0 => Ok(PlayingStatus::Unplayed),
						1 => Ok(PlayingStatus::Playing),
						2 => Ok(PlayingStatus::Played),
						_ => Err(Error::InvalidPlayingStatus),
					}?;

					Ok(())
				}
				Err(rusqlite::Error::QueryReturnedNoRows) => {
					println!("Track not found: {:?}", track);
					Ok(())
				}
				Err(err) => Err(err),
			}?;
		}

		Ok(podcast)
	}

	fn save(
		self: Box<Self>,
		podcasts: &mut dyn Iterator<Item = &'_ Podcast>,
		w: &mut dyn IoWriteSeek,
	) -> BoxResult<()> {
		let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;

		for podcast in podcasts {
			println!("Saving '{}' ({})", podcast.title, podcast.url);
			let id = self.get_podcast(&podcast.title)?;

			for track in podcast.tracks.iter() {
				let playing_status: i32 = match track.playing_status {
					PlayingStatus::Unplayed => 0,
					PlayingStatus::Playing => 1,
					PlayingStatus::Played => 2,
				};

				self.update_episode(&id, &track.url, track.progress, playing_status, now)?;
			}
		}

		// Copy temp file to output
		let mut temp_file = self.db.into_file()?;
		std::io::copy(&mut temp_file, w)?;
		Ok(())
	}
}

impl NewPlayer for PocketCasts {
	fn new(path: &str) -> BoxResult<Box<dyn Player>> {
		Ok(Box::new(Self {
			db: SQLLiteDatabase::open(path)?,
		}))
	}

	fn name() -> &'static str {
		"Pocket Casts"
	}
	fn cli_name() -> &'static str {
		"pocketcasts"
	}
}
