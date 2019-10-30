mod beyondpod;
mod pocketcasts;

use crate::podcast::Podcast;
use crate::BoxResult;

pub use beyondpod::BeyondPod;
pub use pocketcasts::PocketCasts;

pub trait IoWriteSeek: std::io::Write + std::io::Seek {}
impl<T> IoWriteSeek for T where T: std::io::Write + std::io::Seek {}

pub trait NewPlayer: Player {
	fn new(path: &str) -> BoxResult<Box<dyn Player>>;
	fn name() -> &'static str;
	fn cli_name() -> &'static str;
}

pub trait Player {
	fn populate(&mut self, podcast: Podcast) -> BoxResult<Podcast>;
	fn save(
		self: Box<Self>,
		podcasts: &mut dyn Iterator<Item = &'_ Podcast>,
		w: &mut dyn IoWriteSeek,
	) -> BoxResult<()>;
}
