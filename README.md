# Podcast History Converter

## Dependancies

- libsqlite3-dev

		sudo apt install libsqlite3-dev
	
- Rust, Cargo, etc.

## Tested with

- Rust: 1.38.0
- BeyondPod: v4.2.41
- Pocket Casts: 7.5.3

## How to use

1. Ensure all feeds are up to date in the source player
1. Export the OPML file from the source player
1. Import the OPML file into the destination player

	- Best results are acheived when using a completely clean installation of the destination player

1. Export the OPML file from the destination player

	- Compare the two OPML files and ensure they contain all the same feeds 

1. Copy the save files for both the source and destination players

	- See [How to get the save files](#how-to-get-the-save-files)

1. Run `podcast_history_converter` inputing these collected files

	- See [Example CLI usage](#example-cli-usage) 

1. Copy output file back on to the phone

## Example CLI usage

### Files

	- `podcasts_opml.xml`: OPML file of the feeds to be converted
	- `BeyondPod_Backup_YYYY-MM-DD.bpbak`: BeyondPod save file
	- `pocketcasts`: Pocket Casts save file

### Convert from BeyondPod to Pocket Casts

	podcast_history_converter --opml podcasts_opml.xml --beyondpod BeyondPod_Backup_YYYY-MM-DD.bpbak --pocketcasts pocketcasts --in-beyondpod --out-pocketcasts pocketcasts_new

This creates the output file `pocketcasts_new` which can then be copied back onto the phone:

	adb root
	adb push pocketcasts_new /data/data/au.com.shiftyjelly.pocketcasts/databases/pocketcasts
	adb shell
	cd /data/data/au.com.shiftyjelly.pocketcasts/databases
	chmod 660 pocketcasts
	chown <user>:<group> pocketcasts

### Convert from Pocket Casts to BeyondPod 

	podcast_history_converter --opml podcasts_opml.xml --beyondpod BeyondPod_Backup_YYYY-MM-DD.bpbak --pocketcasts pocketcasts --in-pocketcasts --out-beyondpod BeyondPod_Backup_YYYY-MM-DD-1.bpbak

This creates the output file `BeyondPod_Backup_YYYY-MM-DD-1.bpbak` which can then be copied back onto the phone and restored from the BeyondPod settings. 

## How to get the save files

### OPML

The OPML file can be exported from the settings menu of the podcast player.

### Pocket Casts

This requires a rooted android phone.
Although this phone does not need to be the phone you use normally as Pocket Casts will automatically sync the devices.

The `pocketcasts` file is stored at: `/data/data/au.com.shiftyjelly.pocketcasts/databases/pocketcasts`.

This file can be pulled by adb:

	adb root
	adb pull /data/data/au.com.shiftyjelly.pocketcasts/databases/pocketcasts .

### BeyondPod

1. Create backup

	- Settings -> Backup and Restore -> Backup

1. Use 'Share Backup' to get the backup file
