use crate::{BoxResult, Error};

use std::path::Path;

use reqwest::Url;
use roxmltree::Node;

fn find_child<'a>(node: Node<'a, 'a>, child: &'static str) -> Result<Node<'a, 'a>, Error> {
	node.children()
		.find(|n| n.is_element() && n.tag_name().name() == child)
		.ok_or(Error::MissingXMLNode(child))
}

pub fn from_opml<P: AsRef<Path>>(path: P) -> BoxResult<Vec<Podcast>> {
	let opml_str = std::fs::read_to_string(path)?;
	let doc = roxmltree::Document::parse(opml_str.as_str())?;

	find_child(doc.root_element(), "body")? // body node
		.children()
		.filter(|n| n.is_element() && n.tag_name().name() == "outline") // all category nodes
		.map(|category| {
			category
				.children()
				.filter(|n| n.is_element() && n.tag_name().name() == "outline") // all feed nodes in this category
				.filter_map(|feed| {
					Some(Podcast::new(
						feed.attribute("xmlUrl")?,
						feed.attribute("text")?,
					))
				})
		})
		.flatten()
		.collect()
}

#[derive(Debug)]
pub struct Podcast {
	pub url: Url,
	pub title: String,
	pub tracks: Vec<Track>,
}

impl Podcast {
	pub fn new(url: &str, title: &str) -> BoxResult<Self> {
		let url = Url::parse(url)?;
		println!("Fetching '{}' ({})", title, url);

		let feed_body = reqwest::get(url.clone())?.text()?;
		let doc = roxmltree::Document::parse(feed_body.as_str())?;

		let tracks = find_child(doc.root_element(), "channel")? // channel node
			.children()
			.filter(|item| item.is_element() && item.tag_name().name() == "item") // all item nodes
			.filter_map(Podcast::track_subnodes_from_item)
			.filter_map(Track::from_subnodes)
			.collect();

		Ok(Self {
			url: url,
			title: title.into(),
			tracks: tracks,
		})
	}

	fn track_subnodes_from_item<'a>(item: Node<'a, 'a>) -> Option<(Node, Node, Option<Node>)> {
		let guid = item
			.children()
			.find(|n| n.is_element() && n.tag_name().name() == "guid")?;
		let enclosure = item
			.children()
			.find(|n| n.is_element() && n.tag_name().name() == "enclosure")?;

		let duration = item.children().find(|n| {
			n.is_element()
				&& n.tag_name().name() == "duration"
				&& n.tag_name()
					.namespace()
					.and_then(|uri| n.lookup_prefix(uri))
					.map_or_else(|| false, |prefix| prefix == "itunes")
		});

		Some((guid, enclosure, duration))
	}
}

#[derive(Debug, PartialEq)]
pub enum PlayingStatus {
	Unplayed,
	Playing,
	Played,
}

#[derive(Debug)]
pub struct Track {
	pub guid: String,
	pub url: Url,
	pub duration: Option<i32>,

	pub progress: i32,
	pub playing_status: PlayingStatus,
}

impl Track {
	fn new(guid: String, url: Url, duration: Option<i32>) -> Self {
		Self {
			guid: guid,
			url: url,
			duration: duration,
			progress: 0,
			playing_status: PlayingStatus::Unplayed,
		}
	}

	fn from_subnodes((guid, url, duration): (Node, Node, Option<Node>)) -> Option<Self> {
		Some(Self::new(
			guid.text()?.into(),
			Url::parse(url.attribute("url")?).ok()?,
			duration
				.and_then(|n| n.text())
				.and_then(Track::duration_from_str),
		))
	}

	pub fn duration_from_str(dur_text: &str) -> Option<i32> {
		let mut dur_split = dur_text
			.split(':')
			.take(3)
			.map(|s| u32::from_str_radix(s, 10).map(|u| u as i32))
			.collect::<Result<Vec<i32>, _>>()
			.ok()?;
		dur_split.reverse();

		if dur_split.is_empty() {
			return None;
		}

		let mut dur = dur_split[0];

		if dur_split.len() == 1 {
			return Some(dur);
		}

		if dur >= 60 {
			return None;
		}

		dur = 60 * dur_split[1] + dur;

		if dur_split.len() == 2 {
			return Some(dur);
		}

		if dur >= 3600 {
			None
		} else {
			Some(3600 * dur_split[2] + dur)
		}
	}
}
