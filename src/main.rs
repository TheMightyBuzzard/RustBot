#![allow(unused_mut)]
#![allow(unused_must_use)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_assignments)]
#![allow(non_snake_case)]
extern crate curl;
extern crate irc;
extern crate rusqlite;
extern crate rustc_serialize;
extern crate regex;
extern crate time;
extern crate rand;
//extern crate crypto;
extern crate rss;
extern crate atom_syndication;

use std::env;
use std::thread;
use std::process::exit;
use std::str;
use std::fs::File;
use std::io::BufReader;
use std::io::BufRead;
use std::io::Write;
use std::fs::OpenOptions;
use std::sync::mpsc::Sender;
use std::sync::mpsc;
use std::time::Duration;
use std::i64;
use regex::Regex;
use curl::easy::{Easy, List};
use irc::client::prelude::*;
use rustc_serialize::json::Json;
use rusqlite::Connection;
use rand::Rng;
use rss::Rss;
use atom_syndication::Feed;
//use std::collections::HashMap;

#[derive(Debug)]
struct BotConfig {
	nick: String,
	altn1: String,
	altn2: String,
	server: String,
	channel: String,
	prefix: String,
	admin: String,
	snpass: String,
	snuser: String,
	cookie: String,
	wu_key: String,
	go_key: String,
	bi_key: String,
	cse_id: String,
	is_fighting: bool,
}

#[derive(Debug)]
struct CacheEntry {
	age: i64,
	location: String,
	weather: String,
}

struct Storables {
	wucache: Vec<CacheEntry>,
	botconfig: BotConfig,
	server: IrcServer,
	conn: Connection,
	titleres: Vec<Regex>,
	descres: Vec<Regex>,
	channels: Vec<MyChannel>,
}

#[derive(Debug)]
struct MyChannel {
	name: String,
	protected: bool,
}

#[derive(Debug)]
struct Submission {
	reskey: String,
	subject: String,
	story: String,
	chan: String,
	cookie: String,
	botnick: String,
}

#[derive(Debug)]
struct Character {
	nick: String,
	level: u64,
	hp: u64,
	weapon: String,
	armor: String,
	ts: u64,
	initiative: u8,
}


#[derive(Debug)]
enum TimerTypes {
	Message {chan: String, msg: String },
	Action {chan: String, msg: String },
	Recurring { every: i64, command: String },
	Feedback { command: String },
	Sendping { doping: bool },
	Once { command: String },
}

#[derive(Debug)]
struct Timer {
	delay: u64,
	action: TimerTypes,
}

const DEBUG: bool = false;
const ARMOR_CLASS: u8 = 10;

fn main() {
	let args: Vec<_> = env::args().collect();
	if args.len() < 2 {
		println!("Syntax: rustbot botnick");
		exit(0);
	}
	let thisbot = args[1].clone();
	let mut wucache: Vec<CacheEntry> = vec![];
	let conn = Connection::open("/home/bob/etc/snbot/usersettings.db").unwrap();
	if !sql_table_check(&conn, "bot_config".to_string()) {
		println!("`bot_config` table not found, creating...");
		if !sql_table_create(&conn, "bot_config".to_string()) {
			println!("No bot_config table exists and for some reason I cannot create one");
		}
		exit(1);
	}
	if !sql_table_check(&conn, "channels".to_string()) {
		println!("`channels` table not found, creating...");
		if !sql_table_create(&conn, "channels".to_string()) {
			println!("No channels table exists and for some reason I cannot create one");
		}
		exit(1);
	}
	prime_weather_cache(&conn, &mut wucache);
	let botconfig = conn.query_row("SELECT nick, server, channel, prefix, admin_hostmask, snpass, snuser, cookiefile, wu_api_key, google_key, bing_key, g_cse_id FROM bot_config WHERE nick = ?", &[&thisbot], |row| {
			BotConfig {
				nick: row.get(0),
				altn1: row.get(0),
				altn2: row.get(0),
				server: row.get(1),
				channel: row.get(2),
				prefix: row.get(3),
				admin: row.get(4),
				snpass: row.get(5),
				snuser: row.get(6),
				cookie: "".to_string(),
				wu_key: row.get(8),
				go_key: row.get(9),
				bi_key: row.get(10),
				cse_id: row.get(11),
				is_fighting: false,
			}
		}).unwrap();
	let channels = load_channels(&conn);
	let mut vChannels: Vec<String> = Vec::new();
	vChannels.push(botconfig.channel.clone());
	for channel in channels.iter() {
		vChannels.push(channel.name.clone());
	}
	let mut storables: Storables = Storables {
		wucache: wucache,
		conn: Connection::open("/home/bob/etc/snbot/usersettings.db").unwrap(),
		botconfig: conn.query_row("SELECT nick, server, channel, prefix, admin_hostmask, snpass, snuser, cookiefile, wu_api_key, google_key, bing_key, g_cse_id FROM bot_config WHERE nick = ?", &[&thisbot], |row| {
			BotConfig {
				nick: row.get(0),
				altn1: row.get(0),
				altn2: row.get(0),
				server: row.get(1),
				channel: row.get(2),
				prefix: row.get(3),
				admin: row.get(4),
				snpass: row.get(5),
				snuser: row.get(6),
				cookie: "".to_string(),
				wu_key: row.get(8),
				go_key: row.get(9),
				bi_key: row.get(10),
				cse_id: row.get(11),
				is_fighting: false,
			}
			}).unwrap(),
		server: IrcServer::from_config(
				irc::client::data::config::Config {
					owners: None,
					nickname: Some(botconfig.nick.clone()),
					alt_nicks: Some(vec!(botconfig.altn1.clone(), botconfig.altn2.clone())),
					username: Some(botconfig.nick.clone()),
					realname: Some(botconfig.nick.clone()),
					server: Some(botconfig.server.clone()),
					port: Some(6697),
					password: Some(botconfig.snpass.clone()),
					use_ssl: Some(true),
					encoding: Some("UTF-8".to_string()),
					//channels: Some(vec!(botconfig.channel.clone())),
					channels: Some(vChannels),
					channel_keys: None,
					umodes: Some("+Zix".to_string()),
					user_info: Some("MrPlow rewritten in Rust".to_string()),
					ping_time: Some(180),
					ping_timeout: Some(10),
					ghost_sequence: Some(vec!("RECOVER".to_string())),
					should_ghost: Some(true),
					nick_password: Some(botconfig.snpass.clone()),
					options: None
			}).unwrap(),

		titleres: load_titleres(None),
		descres: load_descres(None),
		channels: load_channels(&conn),
	};

	let recurringTimers: Vec<TimerTypes> = get_recurring_timers(&conn);

	storables.server.identify().unwrap();
	conn.close();

	// Feedback channel that any thread can write to?
	let (feedbacktx, feedbackrx) = mpsc::channel::<Timer>();

	// Spin off a submitter listening thread
	let (subtx, subrx) = mpsc::channel::<Submission>();
	{	
		let server = storables.server.clone();
		let substhread = thread::spawn(move || {
			loop {
				for submission in subrx.recv() {
					if DEBUG {
						println!("{:?}", submission);
					}
					thread::sleep(Duration::new(25,0));
					let chan = submission.chan.clone();
					if send_submission(&submission) {
						server.send_privmsg(&chan, "Submission successful. https://soylentnews.org/submit.pl?op=list");
					}
					else {
						server.send_privmsg(&chan, "Something borked during submitting, check the logs.");
					}
				}
			}
		});
	}
	
	// Spin off a timed event thread
	let (timertx, timerrx) = mpsc::channel::<Timer>();
	{
		let server = storables.server.clone();
		let timerthread = thread::spawn(move || {
			let mut qTimers: Vec<Timer> = Vec::new();
			for timer in recurringTimers {
				let mut pushme: Timer;
				match timer {
					TimerTypes::Recurring { ref every, ref command } => {
						let pushme = Timer {
							delay: every.clone() as u64,
							action: TimerTypes::Recurring {
								every: every.clone(),
								command: command.clone(),
							},
						};
						qTimers.push(pushme);
					},
					_ => {},
				};
			}
			let conn = Connection::open("/home/bob/etc/snbot/usersettings.db").unwrap();
			let tenthSecond = Duration::from_millis(100);
			loop {
				match timerrx.try_recv() {
					Err(_) => { },
					Ok(mut timer) => {
						if DEBUG {
							println!("{:?}", timer);
						}
						qTimers.push(timer);
					}
				}
				if !qTimers.is_empty() {
					for timer in qTimers.iter_mut() {
						// First decrement timers
						if timer.delay <= 100_u64 {
							timer.delay = 0_u64;
						}
						else {
							timer.delay = timer.delay - 100_u64;
						}
						
						// Now handle any timers that are at zero
						if timer.delay == 0 {
							timer.delay = handle_timer(&server, &feedbacktx, &conn, &timer.action);
						}
					}
					
					// Drop all timers we've already executed at once to save time
					qTimers.retain(|ref t| t.delay != 0_u64);
				}
				
				thread::sleep(tenthSecond);
			}
		});
	}

	let tGoodfairy = Timer {
		delay: 5000_u64,
		action: TimerTypes::Once {
			command: "goodfairy".to_string(),
		}
	};
	timertx.send(tGoodfairy);

	for message in storables.server.iter() {
		let umessage = message.unwrap();
		let mut chan: String = "foo".to_string();
		let mut said: String = "bar".to_string();
		let msgclone = umessage.clone();
		let nick = msgclone.source_nickname();
		let snick: String;
		match umessage.command {
			irc::client::data::command::Command::PRIVMSG(ref c, ref d) => {
				splitprivmsg(&mut chan, &mut said, &c, &d);
				said = said.trim_right().to_string();
				let hostmask = msgclone.prefix.clone().unwrap().to_string();
				snick = nick.unwrap().to_string();
				println!("{:?}", umessage);

				if check_messages(&storables.conn, &snick) {
					deliver_messages(&storables.server, &storables.conn, &snick);
				}

				if is_action(&said) {
					let mut asaid = said.clone();
					asaid = asaid[8..].to_string();
					let asaidend = asaid.len() - 1;
					asaid = asaid[..asaidend].to_string();
					log_seen(&storables, &chan, &snick, &hostmask, &asaid, 1);
					process_action(&storables, &snick, &chan, &said);
				}
				else if is_command(&mut storables.botconfig.prefix, &said) {
					process_command(&mut storables.titleres, &mut storables.descres, &mut storables.channels, &storables.server, &subtx, &timertx, &storables.conn, &mut storables.wucache, &mut storables.botconfig, &snick, &hostmask, &chan, &said);
					log_seen(&storables, &chan, &snick, &hostmask, &said, 0);
				}
				else {
					log_seen(&storables, &chan, &snick, &hostmask, &said, 0);
					continue;
				}
			},
			irc::client::data::command::Command::PING(_,_) => {continue;},
			irc::client::data::command::Command::PONG(_,_) => {
				match feedbackrx.try_recv() {
					Err(_) => { },
					Ok(timer) => {
						if DEBUG {
							println!("{:?}", timer);
						}
						match timer.action {
							TimerTypes::Feedback {ref command} => {
								match &command[..] {
									"fiteoff" => {
										storables.botconfig.is_fighting = false;
									},
									_ => {},
								};
							},
							_ => {},
						};
						//qTimers.push(timer);
					}	
				};
				println!("{:?}", umessage);
				continue;
			},
			_ => println!("{:?}", umessage)
		}
	}
}

fn splitprivmsg(chan: &mut String, said: &mut String, c: &String, d: &String) {
	*chan = c.clone();
	*said = d.clone();
}

fn is_action(said: &String) -> bool {
	let prefix = "\u{1}ACTION ".to_string();
	let prefixbytes = prefix.as_bytes();
	let prefixlen = prefixbytes.len();
	let saidbytes = said.as_bytes();
	if prefix.len() > said.len() {
		return false;
	}
	let checkbytes = &saidbytes[0..prefixlen];
	if prefixbytes == checkbytes {
		return true;
	}
	false
}

fn is_command(prefix: &String, said: &String) -> bool {
	if said.len() < prefix.len() {
		return false;
	}
	let prefixlen = prefix.len();
	let prefixbytes = prefix.as_bytes();
	let saidbytes = said.as_bytes();
	let checkbytes = &saidbytes[0..prefixlen];
	if prefixbytes == checkbytes {
		return true;
	}
	false
}

fn log_seen(storables: &Storables, chan: &String, snick: &String, hostmask: &String, said: &String, action: i32) {
	let conn = &storables.conn;
	let time: i64 = time::now_utc().to_timespec().sec;
	conn.execute("REPLACE INTO seen VALUES($1, $2, $3, $4, $5, $6)", &[snick, hostmask, chan, said, &time, &action]).unwrap();
}

fn process_action(storables: &Storables, nick: &String, channel: &String, said: &String) {
	let server = &storables.server;
	let prefix = "\u{1}ACTION ".to_string();
	let prefixlen = prefix.len();
	let end = said.len() - 1;
	let csaid: String = said.clone();
	let action: String = csaid[prefixlen..end].to_string();
	if action == "yawns" {
		let flip = format!("flips a Skittle into {}'s gaping mouth", nick);
		server.send_action( channel, &flip );
	}
}

fn cmd_check(checkme: &[u8], against: &str, exact: bool) -> bool {
	if exact {
		if checkme == against.as_bytes() {
			return true;
		}
		return false;
	}
	else {
		let size = against.len();
		if checkme.len() > size {
			if &checkme[..size] == against.as_bytes() {
				return true;
			}
			else {
				return false;
			}
		}
		else {
			return false;
		}
	}
}

fn process_command(mut titleres: &mut Vec<Regex>, mut descres: &mut Vec<Regex>, channels: &mut Vec<MyChannel>, server: &IrcServer, subtx: &Sender<Submission>, timertx: &Sender<Timer>, conn: &Connection, mut wucache: &mut Vec<CacheEntry>, mut botconfig: &mut BotConfig, nick: &String, hostmask: &String, chan: &String, said: &String) {
	let maskonly = hostmask_only(&hostmask);
	let prefix = botconfig.prefix.clone();
	let prefixlen = prefix.len();
	let saidlen = said.len();
	let csaid: String = said.clone();
	let noprefix: String = csaid[prefixlen..saidlen].to_string().trim().to_string();
	let noprefixbytes = noprefix.as_bytes();
	if cmd_check(&noprefixbytes, "quit", true) {
		if !is_admin(&botconfig, &server, &conn, &chan, &maskonly) {
			return;
		}
		command_quit(server, chan.to_string());
		return;
	}
	else if cmd_check(&noprefixbytes, "pissoff", true) {
		if !is_admin(&botconfig, &server, &conn, &chan, &maskonly) {
			return;
		}
		command_pissoff(server, chan.to_string());
		return;
	}
	else if cmd_check(&noprefixbytes, "dieinafire", true) {
		if !is_admin(&botconfig, &server, &conn, &chan, &maskonly) {
			return;
		}
		command_dieinafire(server, chan.to_string());
		return;
	}
	else if cmd_check(&noprefixbytes, "join", true) || cmd_check(&noprefixbytes, "join #", false) {
		if is_abuser(&server, &conn, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "join" {
			command_help(&server, &botconfig, &chan, Some("join".to_string()));
			return;
		}
		let joinchan = noprefix["join ".len()..].trim().to_string();
		command_join(&server, joinchan);
		return;
	}
	else if cmd_check(&noprefixbytes, "seen", true) || cmd_check(&noprefixbytes, "seen ", false) {
		if noprefix.as_str() == "seen" {
			command_help(&server, &botconfig, &chan, Some("seen".to_string()));
			return;
		}
		let who = noprefix["seen ".len()..].trim().to_string();
		command_seen(&server, &conn, &chan, who);
		return;
	}
	else if cmd_check(&noprefixbytes, "smakeadd", true) || cmd_check(&noprefixbytes, "smakeadd ", false) {
		if is_abuser(&server, &conn, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "smakeadd" {
			command_help(&server, &botconfig, &chan, Some("smakeadd".to_string()));
			return;
		}
		let what = noprefix["smakeadd ".len()..].trim().to_string();
		command_smakeadd(&server, &conn, &chan, what);
		return;
	}
	else if cmd_check(&noprefixbytes, "smake", true) || cmd_check(&noprefixbytes, "smake ", false) {
		if noprefix.as_str() == "smake" {
			command_help(&server, &botconfig, &chan, Some("smake".to_string()));
			return;
		}
		let who = noprefix["smake ".len()..].trim().to_string();
		command_smake(&server, &conn, &chan, who);
		return;
	}
	else if cmd_check(&noprefixbytes, "weatheradd", true) || cmd_check(&noprefixbytes, "weatheradd ", false) {
		if noprefix.len() < "weatheradd 12345".len() {
			command_help(&server, &botconfig, &chan, Some("weatheradd".to_string()));
			return;
		}
		let checklocation = noprefix["weatheradd ".len()..].trim().to_string();
		command_weatheradd(&server, &conn, &nick, &chan, checklocation);
		return;
	}
	else if cmd_check(&noprefixbytes, "weather", true) || cmd_check(&noprefixbytes, "weather ", false) {
		if is_abuser(&server, &conn, &chan, &maskonly) {
			return;
		}
		let checklocation: Option<String>;
		if noprefix.as_str() == "weather" {
			checklocation = None;
		}
		else {
			checklocation = Some(noprefix["weather ".len()..].trim().to_string());
		}
		command_weather(&botconfig, &server, &conn, &mut wucache, &nick, &chan, checklocation);
		return;
	}
	else if cmd_check(&noprefixbytes, "abuser", true) || cmd_check(&noprefixbytes, "abuser ", false) {
		if !is_admin(&botconfig, &server, &conn, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "abuser" {
			command_help(&server, &botconfig, &chan, Some("abuser".to_string()));
			return;
		}
		let abuser = noprefix["abuser ".len()..].trim().to_string();
		command_abuser(&server, &conn, &chan, abuser);
		return;
	}
	else if cmd_check(&noprefixbytes, "bot", true) || cmd_check(&noprefixbytes, "bot ", false) {
		if !is_admin(&botconfig, &server, &conn, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "bot" {
			command_help(&server, &botconfig, &chan, Some("bot".to_string()));
			return;
		}
		let bot = noprefix["bot ".len()..].trim().to_string();
		command_bot(&server, &conn, &chan, bot);
		return;
	}
	else if cmd_check(&noprefixbytes, "admin", true) || cmd_check(&noprefixbytes, "admin ", false) {
		if !is_admin(&botconfig, &server, &conn, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "admin" {
			command_help(&server, &botconfig, &chan, Some("admin".to_string()));
			return;
		}
		let admin = noprefix["admin ".len()..].trim().to_string();
		command_admin(&server, &conn, &chan, admin);
		return;
	}
	else if cmd_check(&noprefixbytes, "submit", true) || cmd_check(&noprefixbytes, "submit ", false) {
		if is_abuser(&server, &conn, &chan, &maskonly) {
			return;
		}
		if noprefix.find("http").is_none() {
			command_help(&server, &botconfig, &chan, Some("submit".to_string()));
			return;
		}
		let (suburl, summary) = sub_parse_line(&noprefix);
		command_submit(&mut botconfig, titleres, descres, &server, &chan, &subtx, suburl, summary, &nick);
		return;
	}
	else if cmd_check(&noprefixbytes, "help", true) || cmd_check(&noprefixbytes, "help ", false) {
		let command: Option<String>;
		if noprefix.as_str() == "help" {
			command = None;
		}
		else {
			command = Some(noprefix["help ".len()..].trim().to_string());
		}
		command_help(&server, &botconfig, &chan, command);
		return;
	}
	else if cmd_check(&noprefixbytes, "youtube", true) || cmd_check(&noprefixbytes, "youtube ", false) {
		if is_abuser(&server, &conn, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "youtube" {
			command_help(&server, &botconfig, &chan, Some("youtube".to_string()));
			return;
		}
		let query: String = noprefix["youtube ".len()..].trim().to_string();
		command_youtube(&server, &botconfig, &chan, query);
		return;
	}
	else if cmd_check(&noprefixbytes, "yt", true) || cmd_check(&noprefixbytes, "yt ", false) {
		if is_abuser(&server, &conn, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "yt" {
			command_help(&server, &botconfig, &chan, Some("youtube".to_string()));
			return;
		}
		let query: String = noprefix["yt ".len()..].trim().to_string();
		command_youtube(&server, &botconfig, &chan, query);
		return;
	}
	else if cmd_check(&noprefixbytes, "socialist", true) || cmd_check(&noprefixbytes, "socialist ", false) {
		if noprefix.as_str() == "socialist" {
			command_help(&server, &botconfig, &chan, Some("socialist".to_string()));
			return;
		}
		server.send_privmsg(&chan, format!("{}, you're a socialist!", &noprefix["socialist ".len()..].trim()).as_str());
		return;
	}
	else if cmd_check(&noprefixbytes, "roll", true) || cmd_check(&noprefixbytes, "roll ", false) {
		if noprefix.as_str() == "roll" {
			command_help(&server, &botconfig, &chan, Some("roll".to_string()));
			return;
		}
		let args = noprefix["roll ".len()..].trim().to_string();
		command_roll(&server, &botconfig, &chan, args);
		return;
	}
	else if cmd_check(&noprefixbytes, "bnk", true) {
		server.send_privmsg(&chan, "https://www.youtube.com/watch?v=9upTLWRZTfw");
		return;
	}
	else if cmd_check(&noprefixbytes, "part", true) || cmd_check(&noprefixbytes, "part ", false) {
		if noprefix.as_str() == "part" {
			let partchan = chan.clone();
			command_part(&server, &channels, &chan, partchan);
		}
		else {
			let mut partchan = noprefix["part ".len()..].trim().to_string();
			let sp = partchan.find(" ");
			if sp.is_some() {
				let end = sp.unwrap();
				partchan = partchan[..end].trim().to_string();
			}
			command_part(&server, &channels, &chan, partchan);
		}
		return;
	}
	else if cmd_check(&noprefixbytes, "say", true) || cmd_check(&noprefixbytes, "say ", false) {
		if !is_admin(&botconfig, &server, &conn, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "say" {
			command_help(&server, &botconfig, &chan, Some("say".to_string()));
			return;
		}
		let nocommand = noprefix["say ".len()..].trim().to_string();
		let space = nocommand.find(" ").unwrap_or(0);
		let channel = nocommand[..space].trim().to_string();
		let message = nocommand[space..].trim().to_string();
		command_say(&server, channel, message);
		return;
	}
	else if cmd_check(&noprefixbytes, "tell", true) || cmd_check(&noprefixbytes, "tell ", false) {
		if noprefix.as_str() == "tell" {
			command_help(&server, &botconfig, &chan, Some("tell".to_string()));
			return;
		}
		let space = noprefix.find(" ").unwrap_or(0);
		if space == 0 { return; }
		let nocommand = noprefix[space..].trim().to_string();
		command_tell(&server, &conn, &chan, &nick, nocommand);
		return;
	}
	else if cmd_check(&noprefixbytes, "klingon", true) || cmd_check(&noprefixbytes, "klingon ", false) {
		if noprefix.as_str() == "klingon" {
			command_help(&server, &botconfig, &chan, Some("klingon".to_string()));
			return;
		}
		let english = noprefix["klingon ".len()..].trim().to_string();
		command_klingon(&server, &botconfig, &chan, english);
		return;
	}
	else if cmd_check(&noprefixbytes, "g", true) || cmd_check(&noprefixbytes, "g ", false) {
		if noprefix.as_str() == "g" {
			command_help(&server, &botconfig, &chan, Some("g".to_string()));
			return;
		}
		let searchstr = noprefix["g ".len()..].trim().to_string();
		command_google(&server, &botconfig, &chan, searchstr);
		return;
	}
	else if cmd_check(&noprefixbytes, "fite", true) || cmd_check(&noprefixbytes, "fite ", false) {
		if noprefix.as_str() == "fite" {
			command_help(&server, &botconfig, &chan, Some("fite".to_string()));
			return;
		}
		if botconfig.is_fighting {
			let msg = format!("There's already a fight going on. Wait your turn.");
			server.send_privmsg(&chan, &msg);
			return;
		}
		botconfig.is_fighting = true;
		let target = noprefix["fite ".len()..].trim().to_string();
		let stop = command_fite(&server, &timertx, &conn, &botconfig, &chan, &nick, target);
		// Stop fighting if we didn't actually have a fite
		botconfig.is_fighting = stop;
		if stop {
			fitectl_scoreboard(&server, &conn, &chan, true);
		}
		return;
	}
	else if cmd_check(&noprefixbytes, "fitectl", true) || cmd_check(&noprefixbytes, "fitectl ", false) {
		if noprefix.as_str() == "fitectl" { 
			command_help(&server, &botconfig, &chan, Some("fitectl".to_string()));
			return;
		}
		if botconfig.is_fighting {
			let msg = format!("There's a fight going on. You'll have to wait.");
			server.send_privmsg(&chan, &msg);
			return;
		}
		let args = noprefix["fitectl ".len()..].trim().to_string();
		command_fitectl(&server, &conn, &chan, &nick, args);
		return;
	}
	else if cmd_check(&noprefixbytes, "goodfairy", true) {
		if !is_admin(&botconfig, &server, &conn, &chan, &maskonly) {
                        return;
                }
		command_goodfairy(&server, &conn, &chan);
		return;
	}
	else if cmd_check(&noprefixbytes, "reloadregexes", true) {
		if !is_admin(&botconfig, &server, &conn, &chan, &maskonly) {
			return;
		}
		*titleres = load_titleres(None);
		*descres = load_descres(None);
		return;
	}
	else if cmd_check(&noprefixbytes, "fakeweather", true) || cmd_check(&noprefixbytes, "fakeweather ", false) {
		if is_abuser(&server, &conn, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "sammich" {
			command_help(&server, &botconfig, &chan, Some("sammichadd".to_string()));
			return;
		}
		let sammich = noprefix["sammichadd ".len()..].trim().to_string();
		command_sammichadd(&server, &botconfig, &conn, &chan, sammich);
		return;
	}
	else if cmd_check(&noprefixbytes, "sammich", true) || cmd_check(&noprefixbytes, "sammich ", false) {
		if noprefix.as_str() == "sammich" {
			command_sammich(&server, &botconfig, &conn, &chan, &nick);
		}
		else {
			command_sammich_alt(&server, &chan, &noprefix["sammich ".len()..].trim().to_string());
		}
		return;
	}
	else if cmd_check(&noprefixbytes, "nelson", true) || cmd_check(&noprefixbytes, "nelson ", false) {
		if noprefix.as_str() == "nelson" {
			let message = "HA HA!".to_string();
			command_say(&server, chan.to_string(), message);
		}
		else {
			let target = noprefix["nelson ".len()..].trim().to_string();
			let message = format!("{}: HA HA!", &target);
			command_say(&server, chan.to_string(), message);
		}
		return;
	}
	else if cmd_check(&noprefixbytes, "feedadd", true) || cmd_check(&noprefixbytes, "feedadd ", false) {
		if !is_admin(&botconfig, &server, &conn, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "feedadd" {
			command_help(&server, &botconfig, &chan, Some("feedadd".to_string()));
		}
		else {
			let feed_url = noprefix[7..].to_string().trim().to_string();
			command_feedadd(&server, &botconfig, &conn, &chan, feed_url);
		}
		return;
	}
	else if cmd_check(&noprefixbytes, "fakeweather", true) || cmd_check(&noprefixbytes, "fakeweather ", false) {
		if is_abuser(&server, &conn, &chan, &maskonly) {
                        return;
                }
		if noprefix.as_str() == "fakeweather" {
			command_help(&server, &botconfig, &chan, Some("fakeweather".to_string()));
			return;
		}
                let what = noprefix["fakeweather ".len()..].trim().to_string();
                command_fake_weather_add(&server, &conn, &chan, what, &mut wucache);
		return;
        }
	else if cmd_check(&noprefixbytes, "weatheralias", true) || cmd_check(&noprefixbytes, "weatheralias ", false) {
		if is_abuser(&server, &conn, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "weatheralias" {
	                command_help(&server, &botconfig, &chan, Some("weatheralias".to_string()));
			return;
		}
                let what = noprefix["weatheralias ".len()..].trim().to_string();
                command_weather_alias(&botconfig, &server, &conn, &nick, &chan, what);
		return;
        }
}

fn command_fitectl(server: &IrcServer, conn: &Connection, chan: &String, nick: &String, args: String) {
	let argsbytes = args.as_bytes();
	if args.len() == 10 && &argsbytes[..] == "scoreboard".as_bytes() {
		fitectl_scoreboard(&server, &conn, &chan, false);
	}
	else if args.len() > 7 && &argsbytes[..6] == "armor ".as_bytes() {
		let armor = args[5..].trim().to_string();
		fitectl_armor(&server, &conn, &chan, &nick, armor);
	}
	else if args.len() > 8 && &argsbytes[..7] == "weapon ".as_bytes() {
		let weapon = args[7..].trim().to_string();
		fitectl_weapon(&server, &conn, &chan, &nick, weapon);
	}
	else if args.len() == 6 && &argsbytes[..6] == "status".as_bytes() {
		fitectl_status(&server, &conn, &chan, &nick);
	}
}

fn command_goodfairy(server: &IrcServer, conn: &Connection, chan: &String) {
	conn.execute("UPDATE characters SET hp = level + 10", &[]).unwrap();
	let lucky: String = conn.query_row("SELECT nick FROM characters ORDER BY RANDOM() LIMIT 1", &[], |row| {
		row.get(0)
	}).unwrap();
	conn.execute("UPDATE characters SET hp = level + 100 WHERE nick = ?", &[&lucky]).unwrap();
	server.send_privmsg(&chan, "#fite The good fairy has come along and revived everyone");
	server.send_privmsg(&chan, format!("#fite the gods have smiled upon {}", &lucky).as_str() );
	fitectl_scoreboard(&server, &conn, &chan, true);
}

fn command_fite(server: &IrcServer, timertx: &Sender<Timer>, conn: &Connection, botconfig: &BotConfig, chan: &String, attacker: &String, target: String) -> bool {
	let blocklist = vec!["boru", "Bytram", "bytram"];
	for checknick in blocklist.iter() {
		if **checknick == *target.as_str() {
			server.send_privmsg(&chan, "#fite I'm sorry, Dave, I can't do that.");
			return false;
		}
	}
	if is_nick_here(&server, &chan, &target) {
		if !sql_table_check(&conn, "characters".to_string()) {
			println!("`characters` table not found, creating...");
			if !sql_table_create(&conn, "characters".to_string()) {
				server.send_privmsg(&chan, "No characters table exists and for some reason I cannot create one");
				return false;
			}
		}
		if !character_exists(&conn, &attacker) {
			create_character(&conn, &attacker);
		}
		if !character_exists(&conn, &target) {
			create_character(&conn, &target);
		}

		let returnme = fite(&server, &timertx, &conn, &botconfig, &chan, &attacker, &target);
		return returnme;
	}
	else {
		let err = format!("#fite looks around but doesn't see {}", &target);
		server.send_action(&chan, &err);
		return false;
	}
}

fn command_feedadd(server: &IrcServer, botconfig: &BotConfig, conn: &Connection, chan: &String, feed_url: String) {
	if !sql_table_check(&conn, "feeds".to_string()) {
		println!("`feeds` table not found, creating...");
		if !sql_table_create(&conn, "feeds".to_string()) {
			server.send_privmsg(&chan, "No feeds table exists and for some reason I cannot create one");
			return;
		}
	}

	let raw_feed = get_raw_feed(&feed_url);
	let feed_title = get_feed_title(&raw_feed);
	if &feed_title[..] == "Unknown feed type" {
		server.send_privmsg(&chan, "Unknown feed type. RSS v1.0 maybe?");
		return;
	}
	
	match conn.execute("INSERT INTO feeds (title, address, frequency, lastchecked) VALUES($1, $2, 15, datetime('now', '-16 minutes'))", &[&feed_title, &feed_url]) {
		Err(err) => {
			println!("{}", err);
			server.send_privmsg(&chan, "Error writing to feeds table.");
			return;
		},
		Ok(_) => {
			let sayme: String = format!("\"{}\" added.", feed_url);
			server.send_privmsg(&chan, &sayme);
			return;
		},
	};
}

fn command_sammichadd(server: &IrcServer, botconfig: &BotConfig, conn: &Connection, chan: &String, sammich: String) {
	if !sql_table_check(&conn, "sammiches".to_string()) {
		println!("`sammiches` table not found, creating...");
		if !sql_table_create(&conn, "sammiches".to_string()) {
			server.send_privmsg(&chan, "No sammiches table exists and for some reason I cannot create one");
			return;
		}
	}
	
	match conn.execute("INSERT INTO sammiches VALUES(NULL, $1)", &[&sammich]) {
		Err(err) => {
			println!("{}", err);
			server.send_privmsg(&chan, "Error writing to sammiches table.");
			return;
		},
		Ok(_) => {
			let sayme: String = format!("\"{}\" added.", sammich);
			server.send_privmsg(&chan, &sayme);
			return;
		},
	};
}

fn command_sammich(server: &IrcServer, botconfig: &BotConfig, conn: &Connection, chan: &String, nick: &String) {
	if !sql_table_check(&conn, "sammiches".to_string()) {
		println!("`sammiches` table not found, creating...");
		if !sql_table_create(&conn, "sammiches".to_string()) {
			server.send_privmsg(&chan, "No sammiches table exists and for some reason I cannot create one");
			return;
		}
	}

	let check: i32 = conn.query_row("select count(*) from sammiches", &[], |row| {
		row.get(0)
	}).unwrap();
	if check == 0 {
		server.send_privmsg(&chan, "No sammiches in the database, add some.");
	}

	let result: String = conn.query_row("select sammich from sammiches order by random() limit 1", &[], |row| {
		row.get(0)
	}).unwrap();

	let dome: String = format!("whips up a {} sammich for {}", result, nick);
	server.send_action(&chan, &dome);
}

fn command_sammich_alt(server: &IrcServer, chan: &String, target: &String) {
	if is_nick_here(&server, &chan, &target) {
		let sneak = format!("sneaks up behind {} and cuts their throat", &target);
		let makesammich = format!("fixes thinly sliced {}'s corpse sammiches for everyone in {}", &target, &chan);
		server.send_action(&chan, &sneak.as_str());
		server.send_action(&chan, &makesammich.as_str());
		return;
	}
	else {
		let action = format!("looks around but does not see {}", &target);
		server.send_action(&chan, &action);
		return;
	}
}

fn body_only<'a, 'b>(mut transfer: curl::easy::Transfer<'b, 'a>, dst: &'a mut Vec<u8>) {
	transfer.write_function(move |data: &[u8]| {
		dst.extend_from_slice(data);
		Ok(data.len())
	});
	transfer.perform().unwrap();
}

fn headers_only<'a, 'b>(mut transfer: curl::easy::Transfer<'b, 'a>, dst: &'a mut Vec<u8>) {
	transfer.write_function(nullme).unwrap();
	transfer.header_function(move |data: &[u8]| {
		dst.extend_from_slice(data);
		true
	});
	transfer.perform().unwrap();
}

fn nullme(data: &[u8]) -> Result<usize,curl::easy::WriteError> {
	Ok(data.len())
}

fn command_google(server: &IrcServer, botconfig: &BotConfig, chan: &String, searchstr: String) {
	let mut dst = Vec::new();
	let mut easy = Easy::new();
	let bsearchstr = &searchstr.clone().into_bytes();
	let esearchstr = easy.url_encode(&bsearchstr[..]);
	let bcx = &botconfig.cse_id.clone().into_bytes();
	let ecx = easy.url_encode(&bcx[..]);
	let bkey = &botconfig.go_key.clone().into_bytes();
	let ekey = easy.url_encode(&bkey[..]);
	let url = format!("https://www.googleapis.com/customsearch/v1?q={}&cx={}&safe=off&key={}", esearchstr, ecx, ekey);

	easy.url(url.as_str()).unwrap();
	// Closure so that transfer will go poof after being used
	{
		let mut transfer = easy.transfer();
		body_only(transfer, &mut dst);
	}
	if easy.response_code().unwrap_or(999) != 200 {
		println!("got http response code {} in command_google", easy.response_code().unwrap_or(999));
		return;
	}
	let json = str::from_utf8(&dst[..]).unwrap_or("");
	let jsonthing = Json::from_str(json).unwrap_or(Json::from_str("{}").unwrap());
	let found = jsonthing.find("items");
	if found.is_none() {
		server.send_privmsg(&chan, "sorry, there were no results for your query");
		return;
	}
	let items = found.unwrap();
	let mut resurl = items[0].find("link").unwrap().to_string().trim().to_string();
	let mut ressum = items[0].find("snippet").unwrap().to_string().trim().to_string(); 
	if &resurl[0..1] == "\"" {
		let cresurl = resurl.clone();
		let strresurl = cresurl.as_str();
		let len = strresurl.len() - 1;
		resurl = strresurl[1..len].to_string().trim().to_string();
	}
	let regex = Regex::new(r"\\n").unwrap();
	ressum = regex.replace_all(ressum.as_str(), "");
	let response = format!("{} - {}", resurl, ressum);
	server.send_privmsg(&chan, &response);
}

fn command_klingon(server: &IrcServer, botconfig: &BotConfig, chan: &String, english: String) {
	let token = get_bing_token(&botconfig);
	if token == "".to_string() {
		server.send_privmsg(&chan, "Could not get bing translate token, check the logs");
		return;
	}
	let outlangs = vec!["tlh", "tlh-Qaak"];
	let mut dst = Vec::new();
	let mut translations = Vec::new();
	for lang in outlangs.iter() {
		let mut headerlist = List::new();
		headerlist.append(format!("Authorization: Bearer {}", token).as_str());
		headerlist.append("Accept-Language: en-US");
		headerlist.append("Accept-Charset: utf-8");
		{
			let mut easy = Easy::new();
			let benglish = &english.clone().into_bytes();
			let eenglish = easy.url_encode(&benglish[..]);
			let url = format!("http://api.microsofttranslator.com/V2/Http.svc/Translate?text={}&from=en&to={}&contentType=text/plain", &eenglish, &lang);
			easy.url(url.as_str()).unwrap();
			easy.http_headers(headerlist).unwrap();
			{
				let mut transfer = easy.transfer();
				body_only(transfer, &mut dst);
			}
			easy.perform().unwrap();
			if easy.response_code().unwrap_or(999) != 200 {
				println!("got http response code {} in command_klingon", easy.response_code().unwrap_or(999));
				return;
			}
		}
		let cdst = dst.clone();
		let translation = String::from_utf8(cdst).unwrap_or("".to_string());
		dst = Vec::new();
		translations.push(translation);
	}
	for this in translations.iter() {
		println!("{}", this);
	}
	let reg = Regex::new("^<string.*>(.*?)</string>").unwrap();
	let capone = reg.captures(translations[0].as_str());
	let captwo = reg.captures(translations[1].as_str());
	let tlh = capone.unwrap().at(1).unwrap_or("wtf?!");
	let qaak = captwo.unwrap().at(1).unwrap_or("wtf?!");
	server.send_privmsg(&chan, format!("{} ({})    ", tlh, qaak).as_str());
	return;
}

fn command_tell(server: &IrcServer, conn: &Connection, chan: &String, nick: &String, incoming: String) {
	let space = incoming.find(" ").unwrap_or(0);
	if space == 0 { return; }
	let tellwho = incoming[..space].trim().to_string();
	let tellwhat = incoming[space..].trim().to_string();
	if tellwho.len() < 1 || tellwhat.len() < 1 {
		return;
	}
	if save_msg(&conn, &nick, tellwho, tellwhat) {
		server.send_privmsg(&chan, "Okay, I'll tell them next time I see them.");
	}
	else {
		server.send_privmsg(&chan, "Something borked saving your message, check the logs.");
	}
	return;
}

fn command_roll(server: &IrcServer, botconfig: &BotConfig, chan: &String, args: String) {
	let maxdice = 9000;
	let maxsides = 9000;
	let maxthrows = 13;
	let mut rng = rand::thread_rng();
	let regone = Regex::new("(\\d+)(?i:d)(\\d+)").unwrap();
	let regtwo = Regex::new("throws=(\\d+)").unwrap();
	let captureone = regone.captures(args.as_str());
	let capturetwo = regtwo.captures(args.as_str());
	let mut throws = "1";
	if captureone.is_none() {
		command_help(&server, &botconfig, &chan, Some("roll".to_string()));
		return;
	}
	if capturetwo.is_some() {
		throws = capturetwo.unwrap().at(1).unwrap_or("1");
		println!("throws: {}", throws);
	}
	else {
	}
	let throw: u64 = throws.parse::<u64>().unwrap_or(0) + 1_u64;
	let capture = captureone.unwrap();
	let dices = capture.at(1).unwrap_or("0");
	let dice: u64 = dices.parse::<u64>().unwrap_or(0) + 1_u64;
	let sides = capture.at(2).unwrap_or("0");
	let side: u64 = sides.parse().unwrap_or(0);
	if side > maxsides || dice > maxdice || throw > maxthrows {
		server.send_privmsg(&chan, format!("chromas, is that you? stop being a wiseass.").as_str());
		return;
	}
	else if side < 1 || dice < 1 || throw < 1 {
		server.send_privmsg(&chan, format!("chromas, is that you? stop being a wiseass.").as_str());
                return;
	}

	for pass in 1..throw {
		let mut total: u64 = 0_u64;
		for _ in 1..dice {
			let bignum = rng.gen::<u64>();
			let thisdie = (bignum % side) + 1;
			total += thisdie;
		}
		server.send_privmsg(&chan, format!("pass {}: {}", pass, total).as_str());
	}
	return;
}

fn command_youtube(server: &IrcServer, botconfig: &BotConfig, chan: &String, query: String) {
	let link = get_youtube(&botconfig.go_key, &query);
	server.send_privmsg(&chan, format!("https://www.youtube.com/watch?v={}", link).as_str());
	return;
}

fn command_submit(mut botconfig: &mut BotConfig, mut titleres: &mut Vec<Regex>,mut descres: &mut Vec<Regex>, server: &IrcServer, chan: &String, subtx: &Sender<Submission>, suburl: String, summary: String, submitter: &String) {
	let page: String = sub_get_page(&suburl);
	let title: String = sub_get_title(&mut titleres, &page);
	if title == "".to_string() {
		server.send_privmsg(&chan, "Unable to find a title for that page");
		return;
	}

	let description: String = sub_get_description(&mut descres, &page);
	if description == "".to_string() {
		server.send_privmsg(&chan, "Unable to find a summary for that page");
		return;
	}
	
	let mut cookie = botconfig.cookie.clone();
	if cookie == "".to_string() {
		cookie = sub_get_cookie(&mut botconfig);
	}
	
	let reskey = sub_get_reskey(&cookie);
	if reskey == "".to_string() {
		server.send_privmsg(&chan, "Unable to get a reskey. Check the logs.");
		return;
	}
	
	let story = sub_build_story( &submitter, &description, &summary, &suburl );
	
	let submission = Submission {
		reskey: reskey.clone(),
		subject: title.clone(),
		story: story.clone(),
		chan: chan.clone(),
		cookie: cookie.clone(),
		botnick: botconfig.nick.clone(),
	};

	server.send_privmsg(&chan, "Submitting. There is a mandatory delay, please be patient.");
	
	let foo = subtx.send(submission);
	match foo {
		Ok(_) => {},
		Err(err) => println!("{:?}", err),
	};
}

fn command_quit(server: &IrcServer, chan: String) {
	server.send_privmsg(&chan, "Your wish is my command...");
	server.send_quit("");
}

fn command_pissoff(server: &IrcServer, chan: String) {
	server.send_privmsg(&chan, "Off I shall piss...");
	server.send_quit("");
}

fn command_dieinafire(server: &IrcServer, chan: String) {
	server.send_action(&chan, "dies a firey death");
	server.send_quit("");
}

fn command_join(server: &IrcServer, joinchan: String) {
	server.send_join(&joinchan);
}

fn command_part(server: &IrcServer, vchannels: &Vec<MyChannel>, chan: &String, partchan: String) {
	let botconfig = server.config();
	let channels = botconfig.clone().channels.unwrap();
	let homechannel = channels[0].clone();
	if homechannel.to_string() == partchan {
		let msg = format!("No.");
		server.send_privmsg(&chan, &msg);
		return;
	}
	
	for channel in vchannels.iter() {
		if (channel.name == partchan) && (channel.protected) {
			let msg = format!("No.");
			server.send_privmsg(&chan, &msg);
			return;
		}
	}
	
	// else
	let partmsg: Message = Message {
		tags: None,
		prefix: None,
		command: Command::PART(partchan, None), 
	};
	server.send(partmsg);
	return;
}

fn command_say(server: &IrcServer, chan: String, message: String) {
	server.send_privmsg(&chan, message.as_str());
	return;
}

fn command_seen(server: &IrcServer, conn: &Connection, chan: &String, who: String) {
	struct SeenResult {
		channel: String,
		said: String,
		datetime: String,
		nick: String,
		action: bool
	};
	let result: SeenResult;
	if !sql_table_check(&conn, "seen".to_string()) {
		println!("`seen` table not found, creating...");
		if !sql_table_create(&conn, "seen".to_string()) {
			server.send_privmsg(&chan, "No seen table exists and for some reason I cannot create one");
			return;
		}
	}

	let count: i32 = conn.query_row("SELECT count(nick) FROM seen WHERE nick = ?", &[&who], |row| {
		row.get(0)
	}).unwrap();
	if count == 0 {
		let privmsg = format!("Sorry, I have not seen {}", who);
		server.send_privmsg(&chan, &privmsg);
		return;
	}

	result = conn.query_row("SELECT channel, said, datetime(ts, 'unixepoch'), nick, action FROM seen WHERE nick = ? COLLATE NOCASE ORDER BY ts DESC LIMIT 1", &[&who], |row| {
		SeenResult {
			channel: row.get(0),
			said: row.get(1),
			datetime: row.get(2),
			nick: row.get(3),
			action: match row.get(4) {
				1 => true,
				_ => false
			}
		}
	}).unwrap();
	
	if result.action {
		let privmsg = format!("[{}] {} *{} {}", result.datetime, result.channel, result.nick, result.said);
		server.send_privmsg(&chan, &privmsg);
	}
	else {
		let privmsg = format!("[{}] {} <{}> {}", result.datetime, result.channel, result.nick, result.said);
		server.send_privmsg(&chan, &privmsg);
	}
	return;
}

fn command_smake(server: &IrcServer, conn: &Connection, chan: &String, who: String) {
	if !sql_table_check(&conn, "smakes".to_string()) {
		println!("`smakes` table not found, creating...");
		if !sql_table_create(&conn, "smakes".to_string()) {
			server.send_privmsg(&chan, "No smakes table exists and for some reason I cannot create one");
			return;
		}
	}

	let check: i32 = conn.query_row("select count(*) from smakes", &[], |row| {
		row.get(0)
	}).unwrap();
	if check == 0 {
		server.send_privmsg(&chan, "No smakes in the database, add some.");
	}

	let result: String = conn.query_row("select smake from smakes order by random() limit 1", &[], |row| {
		row.get(0)
	}).unwrap();

	let dome: String = format!("smakes {} upside the head with {}", who, result);
	server.send_action(&chan, &dome);
}

fn command_smakeadd(server: &IrcServer, conn: &Connection, chan: &String, what: String) {
	if !sql_table_check(&conn, "smakes".to_string()) {
		println!("`smakes` table not found, creating...");
		if !sql_table_create(&conn, "smakes".to_string()) {
			server.send_privmsg(&chan, "No smakes table exists and for some reason I cannot create one");
			return;
		}
	}
	
	match conn.execute("INSERT INTO smakes VALUES(NULL, $1)", &[&what]) {
		Err(err) => {
			println!("{}", err);
			server.send_privmsg(&chan, "Error writing to smakes table.");
			return;
		},
		Ok(_) => {
			let sayme: String = format!("\"{}\" added.", what);
			server.send_privmsg(&chan, &sayme);
			return;
		},
	};
}

fn command_fake_weather_add(server: &IrcServer, conn: &Connection, chan: &String, what: String, mut wucache: &mut Vec<CacheEntry>) {
	let mut colon = what.find(':').unwrap_or(what.len());
	if colon == what.len() {
		return;
	}
	let location: String = what[..colon].to_string();
	colon += 1;
	let weather: String = what[colon..].to_string().trim().to_string();
	match conn.execute("INSERT INTO fake_weather VALUES ($1, $2)", &[&location, &weather]) {
		Err(err) => {
                        println!("{}", err);
                        server.send_privmsg(&chan, "Error writing to fake_weather table.");
                        return;
                },
                Ok(_) => {
			let entry: CacheEntry = CacheEntry {
        	                age: std::i64::MAX,
	                        location: location.clone(),
                        	weather: weather.clone(),
                	};
			wucache.push(entry);
                        let sayme: String = format!("\"{}\" added.", location);
                        server.send_privmsg(&chan, &sayme);
                        return;
                },
	};
}

fn command_weather_alias(botconfig: &BotConfig, server: &IrcServer, conn: &Connection, nick: &String, chan: &String, walias: String) {
	if !sql_table_check(&conn, "weather_aliases".to_string()) {
                println!("weather_aliases table not found, creating...");
                if !sql_table_create(&conn, "weather_aliases".to_string()) {
                        server.send_privmsg(&chan, "No weather_aliases table exists and for some reason I cannot create one");
                        return;
                }
        }
	
	let mut colon = walias.find(':').unwrap_or(walias.len());
	if colon == walias.len() {
		command_help(&server, &botconfig, &chan, Some("weatheralias".to_string()));
		return;
	}
	let flocation = walias[..colon].trim().to_string();
	colon += 1;
	let rlocation = walias[colon..].trim().to_string();
	if flocation.len() < 3 || rlocation.len() < 3 {
		command_help(&server, &botconfig, &chan, Some("weatheralias".to_string()));
		return;
	}
	// make sure an alias doesn't stomp on a saved person/place name
	let is_user: i32 = conn.query_row("SELECT count(nick) FROM locations WHERE nick = $1", &[&flocation], |row| {
		row.get(0)
	}).unwrap();
	if is_user != 0 {
		let sayme = format!("{} is someone's nick, jackass.", &flocation);
		server.send_privmsg(&chan, &sayme);
		return;
	}
	match conn.execute("REPLACE INTO weather_aliases VALUES ($1, $2 )", &[&flocation, &rlocation]) {
		Err(err) => {
                        println!("{}", err);
                        server.send_privmsg(&chan, "Error writing to weather_aliases table.");
                        return;
                },
                Ok(_) => {
                        let sayme: String = format!("\"{}\" added.", flocation);
                        server.send_privmsg(&chan, &sayme);
                        return;
                },
	};
}

fn command_weatheradd(server: &IrcServer, conn: &Connection, nick: &String, chan: &String, checklocation: String) {
	
	if !sql_table_check(&conn, "locations".to_string()) {
		println!("locations table not found, creating...");
		if !sql_table_create(&conn, "locations".to_string()) {
			server.send_privmsg(&chan, "No locations table exists and for some reason I cannot create one");
			return;
		}
	}

	match conn.execute("REPLACE INTO locations VALUES($1, $2)", &[nick, &checklocation]) {
		Err(err) => {
			println!("{}", err);
			server.send_privmsg(&chan, "Error saving your location.");
		},
		Ok(_) => {
			let sayme: String = format!("Location for {} set to {}", nick, checklocation);
			server.send_privmsg(&chan, &sayme);
		},
	};
	return;
}

fn command_weather(botconfig: &BotConfig, server: &IrcServer, conn: &Connection, mut wucache: &mut Vec<CacheEntry>, nick: &String, chan: &String, checklocation: Option<String>) {
		let weather: String;
		let mut unaliasedlocation = checklocation;
		let location: Option<String>;

		// unalias unaliasedlocation if it is aliased
		if unaliasedlocation.is_some() {
			let is_alias: i32 = conn.query_row("SELECT count(fake_location) FROM weather_aliases WHERE fake_location = $1", &[&unaliasedlocation.clone().unwrap()], |row| {
				row.get(0)
			}).unwrap();
			if is_alias == 1 {
				unaliasedlocation = Some(conn.query_row("SELECT real_location FROM weather_aliases WHERE fake_location = $1", &[&unaliasedlocation.clone().unwrap()], |row| {
					row.get(0)
				}).unwrap());
			}
		}

		if unaliasedlocation.is_some() {
			let count: i32 = conn.query_row("SELECT count(nick) FROM locations WHERE nick = $1", &[&unaliasedlocation.clone().unwrap()], |row| {
					row.get(0)
			}).unwrap();
			if count == 1 {
				location = Some(conn.query_row("SELECT location FROM locations WHERE nick = $1", &[&unaliasedlocation.clone().unwrap()], |row| {
					row.get(0)
				}).unwrap());
			}
			else {
				location = unaliasedlocation;
			}
		}
		else {		
			let count: i32 = conn.query_row("SELECT count(location) FROM locations WHERE nick = $1", &[nick], |row| {
					row.get(0)
			}).unwrap();
			if count == 0 {
				location = None;
			}
			else {
				location = Some(conn.query_row("SELECT location FROM locations WHERE nick = $1", &[nick], |row| {
					row.get(0)
				}).unwrap());
			}
		}
		
		match location {
			Some(var) =>	weather = get_weather(&mut wucache, &botconfig.wu_key, var.trim().to_string()),
			None => weather = format!("No location found for {}", nick).to_string(),
		};

		server.send_privmsg(&chan, &weather.trim().to_string());
		return;
}

fn command_abuser(server: &IrcServer, conn: &Connection, chan: &String, abuser: String) {
	if hostmask_add(&server, &conn, &chan, "abusers", &abuser) {
		let result: String = format!("Added '{}' to abusers.", &abuser);
		server.send_privmsg(&chan, &result);
	}
	else {
		let result: String = format!("Failed to add '{}' to abusers. Check the logs.", &abuser);
		server.send_privmsg(&chan, &result);
	}
	return;
}

fn command_admin(server: &IrcServer, conn: &Connection, chan: &String, admin: String) {
	if hostmask_add(&server, &conn, &chan, "admins", &admin) {
		let result: String = format!("Added '{}' to admins.", &admin);
		server.send_privmsg(&chan, &result);
	}
	else {
		let result: String = format!("Failed to add '{}' to admins. Check the logs.", &admin);
		server.send_privmsg(&chan, &result);
	}
	return;
}

fn command_bot(server: &IrcServer, conn: &Connection, chan: &String, bot: String) {
	if hostmask_add(&server, &conn, &chan, "bots", &bot) {
		let result: String = format!("Added '{}' to bots.", &bot);
		server.send_privmsg(&chan, &result);
	}
	else {
		let result: String = format!("Failed to add '{}' to bots. Check the logs.", &bot);
		server.send_privmsg(&chan, &result);
	}
	return;
}

fn command_help(server: &IrcServer, botconfig: &BotConfig, chan: &String, command: Option<String>) {
	let helptext: String = get_help(&botconfig.prefix, command);
	server.send_privmsg(&chan, &helptext);
}

fn sql_table_check(conn: &Connection, table: String) -> bool {
	let result: i32 = conn.query_row("SELECT count(name) FROM sqlite_master WHERE type = 'table' and name = ? LIMIT 1", &[&table], |row| {
			row.get(0)
	}).unwrap();
	
	if result == 1 {
		return true;
	}
	false
}

fn sql_table_create(conn: &Connection, table: String) -> bool {
	let schema: String = sql_get_schema(&table);
	match conn.execute(&schema, &[]) {
			Err(err) => { println!("{}", err); return false; },
			Ok(_) => { println!("{} table created", table); return true; },
	};
}

fn prime_weather_cache(conn: &Connection, mut wucache: &mut Vec<CacheEntry>) {
	let table = "fake_weather".to_string();
	if !sql_table_check(&conn, table.clone()) {
                if !sql_table_create(&conn, table.clone()) {
                        println!("Could not create table 'fake_weather'!");
			return;
                }
		return;
        }
	
	let mut statement = format!("SELECT count(*) FROM {}", &table);
        let result: i32 = conn.query_row(statement.as_str(), &[], |row| {
                        row.get(0)
        }).unwrap();
        if result == 0 {
                return;
        }

	statement = format!("SELECT * from {}", &table);
	let mut stmt = conn.prepare(statement.as_str()).unwrap();
	let mut allrows = stmt.query_map(&[], |row| {
		CacheEntry {
                	age: std::i64::MAX,
	                location: row.get(0),
        	        weather: row.get(1),
		}
	}).unwrap();

	for entry in allrows {
		let thisentry = entry.unwrap();
		wucache.push(thisentry);
	}
}

fn check_messages(conn: &Connection, nick: &String) -> bool {
	let table = "messages".to_string();
	if !sql_table_check(&conn, table.clone()) {
		return false;
	}
	let statement: String = format!("SELECT count(*) FROM {} WHERE recipient = $1", &table);
	let result: i32 = conn.query_row(statement.as_str(), &[&nick.as_str()], |row| {
			row.get(0)
	}).unwrap();
	if result > 0 {
		return true;
	}
	false
}

fn deliver_messages(server: &IrcServer, conn: &Connection, nick: &String) {
	struct Row {
		sender: String,
		message: String,
		ts: i64
	};
	let mut timestamps: Vec<i64> = vec![];
	let mut stmt = conn.prepare(format!("SELECT * FROM messages WHERE recipient = '{}' ORDER BY ts", &nick).as_str()).unwrap();
	let mut allrows = stmt.query_map(&[], |row| {
		Row {
			sender: row.get(0),
			message: row.get(2),
			ts: row.get(3)
		}
	}).unwrap();

	for row in allrows {
		let thisrow = row.unwrap();
		server.send_privmsg(&nick, format!("<{}> {}", thisrow.sender, thisrow.message).as_str());
		timestamps.push(thisrow.ts);
	}
	
	for ts in timestamps.iter() {
		let statement = format!("DELETE FROM messages WHERE recipient = '{}' AND ts = {}", &nick, &ts);
		match conn.execute(statement.as_str(), &[]) {
			Err(err) => println!("{}", err),
			Ok(_) => {}
		};
	}
	return;
}

fn save_msg(conn: &Connection, fromwho: &String, tellwho: String, tellwhat: String) -> bool {
	let table = "messages".to_string();
	if !sql_table_check(&conn, table.clone()) {
		if !sql_table_create(&conn, table.clone()) {
			return false;
		}
	}
	
	let time: i64 = time::now_utc().to_timespec().sec;
	let statement: String = format!("INSERT INTO {} VALUES($1, $2, $3, $4)", &table).to_string();
	match conn.execute(statement.as_str(), &[&fromwho.as_str(), &tellwho, &tellwhat, &time]) {
		Err(err) => {
			println!("{}", err);
			return false;
		},
		Ok(_) => return true,
	};
}

fn get_help(prefix: &String, command: Option<String>) -> String {
	if command.is_none() {
		return "Commands: help, weatheradd, weather, submit, seen, smake, smakeadd, youtube, abuser, bot, admin, socialist, roll, bnk, join, part, tell, klingon, g, sammich, sammichadd, say, pissoff, dieinafire, quit, nelson".to_string();
	}
	let inside = command.unwrap();
	match &inside[..] {
		"help" => format!("Yes, recursion is nifty. Now piss off."),
		"weatheradd" => format!("{}weatheradd <zip> or {}weatheradd city, st", prefix, prefix),
		"weather" => format!("{}weather <zip>, {}weather city, st, or just {}weather if you have saved a location with {}weatheradd", prefix, prefix, prefix, prefix),
		"fakeweather" => format!("{}fakeweather <fakelocation>:<fake weather>", prefix),
		"weatheralias" => format!("{}weatheralias <alias>:<location>", prefix),
		"submit" => format!("{}submit <url> or {}submit <url> <what you have to say about it>", prefix, prefix),
		"seen" => format!("{}seen <nick>", prefix),
		"smake" => format!("{}smake <someone>", prefix),
		"smakeadd" => format!("{}smakeadd <something to smake people with> e.g. {}smakeadd a half-brick in a sock", prefix, prefix),
		"abuser" => format!("limits the commands a jackass can use. {}abuser <full @hostmask> e.g. {}abuser @Soylent/Staff/Editor/cmn32480", prefix, prefix),
		"bot" => format!("registers a hostmask as a bot. {}bot <full @hostmask> e.g. {}bot @universe2.us/ircbot/aqu4", prefix, prefix),
		"admin" => format!("give someone godlike powers over this bot. {}admin <full @hostmask> e.g. {}admin @Soylent/Staff/Developer/TMB", prefix, prefix),
		"youtube" => format!("search youtube. {}youtube <search string>", prefix),
		"socialist" => format!("has half of a libertarian debate. {}socialist <nick>", prefix),
		"roll" => format!("roll the dice. {}roll 3d6 throws=6", prefix),
		"bnk" => format!("BOOBIES n KITTEHS!"),
		"join" => format!("{}join <channel>", prefix),
		"part" => format!("{}part or {}part <channel>", prefix, prefix),
		"tell" => format!("{}tell <nick> <message>", prefix),
		"klingon" => format!("translate something to klingon. {}klingon <phrase>", prefix),
		"g" => format!("google search. {}g <search query>", prefix),
		"sammich" => format!("no need for sudo..."),
		"sammichadd" => format!("add a sammich to the db. {}sammichadd <type of sammich>", prefix),
		"say" => format!("{}say <channel/nick> <stuff>", prefix),
		"pissoff" => format!("alias for {}quit", prefix),
		"dieinafire" => format!("alias for {}quit", prefix),
		"quit" => format!("pretty self-explanatory"),
		"reloadregexes" => format!("reloads the regexes for matching title and description of a page for {}submit from disk", prefix),
		"nelson" => format!("{}nelson <with or without a nick>", prefix),
		_ => format!("{}{} is not a currently implemented command", prefix, inside),
	}
}

fn sql_get_schema(table: &String) -> String {
	match &table[..] {
		"seen" => "CREATE TABLE seen(nick TEXT, hostmask TEXT, channel TEXT, said TEXT, ts UNSIGNED INT(8), action UNSIGNED INT(1) CHECK(action IN(0,1)), primary key(nick, channel) )".to_string(),
		"smakes" => "CREATE TABLE smakes (id INTEGER PRIMARY KEY AUTOINCREMENT, smake TEXT NOT NULL)".to_string(),
		"sammiches" => "CREATE TABLE sammiches (id INTEGER PRIMARY KEY AUTOINCREMENT, sammich TEXT NOT NULL)".to_string(),
		"bot_config" => "CREATE TABLE bot_config(nick TEXT PRIMARY KEY, server TEXT, channel TEXT, perl_file TEXT, prefix TEXT, admin_hostmask TEXT, snpass TEXT, snuser TEXT, cookiefile TEXT, wu_api_key TEXT, google_key text, bing_key TEXT, g_cse_id TEXT)".to_string(),
		"channels" => "CREATE TABLE channels (name TEXT PRIMARY KEY, protected UNSIGNED INT(2) NOT NULL DEFAULT 0)".to_string(),
		"locations" => "CREATE TABLE locations(nick TEXT PRIMARY KEY, location TEXT)".to_string(),
		"bots" => "CREATE TABLE bots(hostmask TEXT PRIMARY KEY NOT NULL)".to_string(),
		"abusers" => "CREATE TABLE abusers(hostmask TEXT PRIMARY KEY NOT NULL)".to_string(),
		"admins" => "CREATE TABLE admins(hostmask PRIMARY KEY NOT NULL)".to_string(),
		"test" => "CREATE TABLE test(hostmask PRIMARY KEY NOT NULL)".to_string(),
		"messages" => "CREATE TABLE messages(sender TEXT, recipient TEXT, message TEXT, ts UNSIGNED INT(8))".to_string(),
		"feeds" => "CREATE TABLE feeds(id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT, address TEXT NOT NULL, frequency INTEGER, lastchecked TEXT)".to_string(),
		"feed_items" => "CREATE TABLE feed_items(feed_id INTEGER, md5sum TEXT, PRIMARY KEY (feed_id, md5sum))".to_string(),
		"fake_weather" => "CREATE TABLE fake_weather(location TEXT PRIMARY KEY NOT NULL, forecast TEXT NOT NULL)".to_string(),
		"weather_aliases" => "CREATE TABLE weather_aliases(fake_location TEXT PRIMARY KEY NOT NULL, real_location TEXT NOT NULL)".to_string(),
		"characters" => "CREATE TABLE characters(nick TEXT PRIMARY KEY NOT NULL, level UNSIGNED INT(8), hp UNSIGNED INT(8), weapon TEXT NOT NULL DEFAULT 'fist', armor TEXT NOT NULL DEFAULT 'grungy t-shirt', ts UNSIGNED INT(8))".to_string(),
		_ => "".to_string(),
	}
}

fn cache_push(mut cache: &mut Vec<CacheEntry>, location: &String, weather: &String) {
	cache_prune(&mut cache);
	let entry = CacheEntry {
		age: time::now_utc().to_timespec().sec,
		location: location.to_string().clone().to_lowercase(),
		weather: weather.to_string().clone().to_lowercase(),
	};
	cache.push(entry);
	return;
}

fn cache_dump(cache: Vec<CacheEntry>) {
	println!("{:?}", cache);
}

fn cache_get(mut cache: &mut Vec<CacheEntry>, location: &String) -> Option<String> {
	cache_prune(&mut cache);
	let position: Option<usize> = cache.iter().position(|ref x| x.location == location.to_string().clone().to_lowercase());
	if position.is_some() {
		let weather: &String = &cache[position.unwrap()].weather;
		return Some(weather.to_string().clone());
	}
	None
}

fn cache_prune(mut cache: &mut Vec<CacheEntry>) {
	if cache.is_empty() { return; }
	let oldest: i64 = time::now_utc().to_timespec().sec - 14400;
	loop {
		let position = cache.iter().position(|ref x| x.age < oldest);
		match position {
			Some(var) => cache.swap_remove(var),
			None => break,
		};
	}
	cache.shrink_to_fit();
}

fn get_weather(mut wucache: &mut Vec<CacheEntry>, wu_key: &String, location: String) -> String {
	let cached = cache_get(&mut wucache, &location);
	if cached.is_some() {
		return cached.unwrap();
	}

	let mut dst = Vec::new();
	let mut easy = Easy::new();
	let encloc = fix_location(&location).to_string();
	let url = format!("http://api.wunderground.com/api/{}/forecast/q/{}.json", wu_key.to_string(), encloc.to_string());
	easy.url(url.as_str()).unwrap();
	{
		let mut transfer = easy.transfer();
		body_only(transfer, &mut dst);
	}
	if easy.response_code().unwrap_or(999) != 200 {
		return format!("got http response code {}", easy.response_code().unwrap_or(999)).to_string();
	}

	let json = str::from_utf8(&dst[..]).unwrap_or("");
	let jsonthing = Json::from_str(json).unwrap_or(Json::from_str("{}").unwrap());
	let forecast;
	let path = jsonthing.find_path(&["forecast", "txt_forecast", "forecastday" ]);
	if path.is_some() {
		forecast = path.unwrap();
	}
	else {
		return "Unable to find weather for that location".to_string();
	}
	let today = forecast[0].find_path(&["fcttext"]).unwrap().as_string().unwrap();
	let tomorrow = forecast[2].find_path(&["fcttext"]).unwrap().as_string().unwrap();
	let forecast = format!("Today: {} Tomorrow: {}", today, tomorrow);
	cache_push(&mut wucache, &location, &forecast);
	return forecast;
}

fn fix_location(location: &String) -> String {
	let numeric = location.parse::<u32>();
	
	if numeric.is_ok() {
		let zip: String = numeric.unwrap().to_string();
		return zip;
	}

	let comma = location.find(',').unwrap_or(location.len());
	if comma < location.len() {
		let location = location.clone();
		let (city, state) = location.split_at(comma);
		let mut easy = Easy::new();
		let citybytes = city.clone().to_string().into_bytes();
		let statebytes = state.clone().to_string().into_bytes();
		let enccity = easy.url_encode(&citybytes[..]);
		let encstate = easy.url_encode(&statebytes[..]);
		let citystate = format!("{}/{}", encstate.trim_left_matches(",").trim(), enccity.trim()).to_string();
		return citystate;
	}
	else {
		let failed = "dohelp".to_string();
		return failed;
	}
}

fn hostmask_add(server: &IrcServer, conn: &Connection, chan: &String, table: &str, hostmask: &String) -> bool {
	if !sql_table_check(&conn, table.to_string()) {
		println!("{} table not found, creating...", table);
		if !sql_table_create(&conn, table.to_string()) {
			let err: String = format!("No {} table exists and for some reason I cannot create one. Check the logs.", table);
			server.send_privmsg(&chan, &err);
			return false;
		}
	}
	
	let statement: String = format!("INSERT INTO {} VALUES($1)", &table).to_string();
	match conn.execute(statement.as_str(), &[&hostmask.as_str()]) {
		Err(err) => {
			println!("{}", err);
			return false;
		},
		Ok(_) => return true,
	};
}

fn is_admin(botconfig: &BotConfig, server: &IrcServer, conn: &Connection, chan: &String, hostmask: &String) -> bool {
	let table = "admins".to_string();
	if !sql_table_check(&conn, table.clone()) {
		println!("{} table not found, creating...", &table);
		if !sql_table_create(&conn, table.clone()) {
			let err: String = format!("No {} table exists and for some reason I cannot create one. Check the logs.", &table);
			server.send_privmsg(&chan, &err);
			return false;
		}
		let statement: String = format!("INSERT INTO {} VALUES($1)", &table).to_string();
		match conn.execute(statement.as_str(), &[&botconfig.admin.as_str()]) {
			Err(err) => {
				println!("{}", err);
				return false;
			},
			Ok(_) => {},
		};
	}
	
	let statement: String = format!("SELECT count(*) FROM {} WHERE hostmask = $1", &table);
	let hostmask: String = hostmask_only(&hostmask);
	let result: i32 = conn.query_row(statement.as_str(), &[&hostmask], |row| {
			row.get(0)
	}).unwrap();
	if result == 1 {
		return true;
	}
	false
}

fn is_bot(server: &IrcServer, conn: &Connection, chan: &String, hostmask: &String) -> bool {
	let table = "bots".to_string();
	if !sql_table_check(&conn, table.clone()) {
		println!("{} table not found, creating...", &table);
		if !sql_table_create(&conn, table.clone()) {
			let err: String = format!("No {} table exists and for some reason I cannot create one. Check the logs.", &table);
			server.send_privmsg(&chan, &err);
			return false;
		}
	}
	
	let statement: String = format!("SELECT count(*) FROM {} WHERE hostmask = $1", &table);
	let hostmask: String = hostmask_only(&hostmask);
	let result: i32 = conn.query_row(statement.as_str(), &[&hostmask], |row| {
			row.get(0)
	}).unwrap();
	if result == 1 {
		return true;
	}
	false
}

fn is_abuser(server: &IrcServer, conn: &Connection, chan: &String, hostmask: &String) -> bool {
	let table = "abusers".to_string();
	if !sql_table_check(&conn, table.clone()) {
		println!("{} table not found, creating...", &table);
		if !sql_table_create(&conn, table.clone()) {
			let err: String = format!("No {} table exists and for some reason I cannot create one. Check the logs.", &table);
			server.send_privmsg(&chan, &err);
			return false;
		}
	}
	
	let statement: String = format!("SELECT count(*) FROM {} WHERE hostmask = $1", &table);
	let hostmask: String = hostmask_only(&hostmask);
	let result: i32 = conn.query_row(statement.as_str(), &[&hostmask], |row| {
			row.get(0)
	}).unwrap();
	if result == 1 {
		return true;
	}
	false
}

fn hostmask_only(fullstring: &String) -> String {
	let position: Option<usize> = fullstring.as_str().find("@");
	if position.is_some() {
		let here = position.unwrap();
		let maskonly = fullstring[here..].to_string();
		return maskonly;
	}
	"OMGWTFBBQ".to_string()
}

fn sub_parse_line(noprefix: &String) -> (String, String) {
	let http: Option<usize> = noprefix[..].to_string().trim().to_string().find("http");
	let mut suburl: String = " ".to_string();
	let mut summary: String = " ".to_string();
	if http.is_some() {
		let beginurl = http.unwrap();
		let preurl = noprefix[beginurl..].to_string().trim().to_string();
		let space: Option<usize> = preurl.find(" ");
		if space.is_some() {
			let sp = space.unwrap();
			suburl = preurl[..sp].to_string().trim().to_string();
			summary = preurl[sp..].to_string().trim().to_string();
		}
		else {
			suburl = preurl;
		}
	}
	else {
		println!("http not found");
	}
	(suburl, summary)
}

fn sub_get_page(url: &String) -> String {
	let mut dst = Vec::new();
	let mut easy = Easy::new();
	easy.url(url.as_str()).unwrap();
	{
		let mut transfer = easy.transfer();
		body_only(transfer, &mut dst);
	}
	if easy.response_code().unwrap_or(999) != 200 {
		return format!("got http response code {}", easy.response_code().unwrap_or(999));
	}

	let page = str::from_utf8(&dst[..]).unwrap_or("");
	return page.to_string().trim().to_string();
}

fn sub_get_title(titleres: &Vec<Regex>, page: &String) -> String {
	let mut title = "".to_string();
	for regex in titleres.iter() {
		let captures = regex.captures(page.as_str());
		if captures.is_none() {
			title = "".to_string();
		}
		else {
			let cap = captures.unwrap().at(1).unwrap();
			title = cap.to_string().trim().to_string();
			break;
		}
	}
	return title;
}

fn sub_get_description(descres: &Vec<Regex>, page: &String) -> String {
	let mut desc = "".to_string();
	for regex in descres.iter() {
		let captures = regex.captures(page.as_str());
		if captures.is_none() {
			desc = "".to_string();
		}
		else {
			let unwrapped = captures.unwrap();
			let cap = unwrapped.at(1).unwrap();
			desc = cap.to_string().trim().to_string();
			break;
		}
	}	
	return desc;
}

//let story = sub_build_story( &submitter, &description, &summary )
fn sub_build_story(submitter: &String, description: &String, summary: &String, source: &String) -> String {
	let story = format!("Submitted via IRC for {}<blockquote>{}</blockquote>{}\n\nSource: {}", submitter, description, summary, source).to_string();
	return story;
}

fn sub_get_reskey(cookie: &String) -> String {
	let url: String;
	if DEBUG {
		url = "https://dev.soylentnews.org/api.pl?m=story&op=reskey".to_string().trim().to_string();
	}
	else {
		url = "https://soylentnews.org/api.pl?m=story&op=reskey".to_string().trim().to_string();
	}

	let mut dst = Vec::new();
	let mut easy = Easy::new();
	easy.url(url.as_str()).unwrap();
	easy.cookie(cookie.as_str()).unwrap();
	{
		let mut transfer = easy.transfer();
		body_only(transfer, &mut dst);
	}
	if easy.response_code().unwrap_or(999) != 200 {
		println!("got http response code {}", easy.response_code().unwrap_or(999));
		return "".to_string();
	}
	
	let unparsed = str::from_utf8(&dst[..]).unwrap_or("");
	let jsonthing = Json::from_str(unparsed).unwrap_or(Json::from_str("{}").unwrap());
	let resopt = jsonthing.find("reskey");
	let mut reskey: String;
	if resopt.is_some() {
		reskey = resopt.unwrap().to_string().trim().to_string();
		let creskey = reskey.clone();
		let strreskey = creskey.as_str();
		let len = strreskey.len() - 1;
		reskey = strreskey[1..len].to_string().trim().to_string();
	}
	else {
		reskey = "".to_string();
	}
	return reskey;
}

fn sub_get_cookie(botconfig: &mut BotConfig) -> String {
	if botconfig.cookie != "".to_string() {
		return botconfig.cookie.clone();
	}
	let url: String;
	if DEBUG {
		url = format!("https://dev.soylentnews.org/api.pl?m=auth&op=login&nick={}&pass={}", "MrPlow", &botconfig.snpass).to_string();
	}
	else {
		url = format!("https://soylentnews.org/api.pl?m=auth&op=login&nick={}&pass={}", &botconfig.nick, &botconfig.snpass).to_string();
	}
	let mut dst = Vec::new();
	let mut easy = Easy::new();
	easy.url(url.as_str()).unwrap();
	{
		let mut transfer = easy.transfer();
		headers_only(transfer, &mut dst);
	}
	if easy.response_code().unwrap_or(999) != 200 {
		println!("got http response code {}", easy.response_code().unwrap_or(999));
		return "".to_string();
	}

	let headers = str::from_utf8(&dst[..]).unwrap_or("").split("\n");
	let mut cookie: String = "".to_string();
	for foo in headers {
		if foo.find("Set-Cookie:").unwrap_or(22) != 22 {
			cookie = foo[12..].to_string().trim().to_string();
			let ccookie = cookie.clone();
			let strcookie = ccookie.as_str();
			let end = strcookie.find("path=/;").unwrap_or(22_usize) + 7;
			cookie = strcookie[..end].to_string().trim().to_string();
		}
	}
	if cookie != "".to_string() {
		botconfig.cookie = cookie.clone();
	}
	return cookie;
}

fn load_titleres(exists: Option<Vec<Regex>>) -> Vec<Regex> {
	let titleresf = OpenOptions::new().read(true).write(true).create(true).open("/home/bob/etc/snbot/titleres.txt");
	if titleresf.is_err() {
		println!("Error opening titleres.txt: {:?}", titleresf);
		let wtf: Vec<Regex> = Vec::new();
		return wtf;
	}
	let unwrapped = &titleresf.unwrap();
	let titleresfile = BufReader::new(unwrapped);

	match exists {
		None => {
			let mut titleres: Vec<Regex> = Vec::new();
			for line in titleresfile.lines() {
				if line.is_ok() {
					titleres.push(Regex::new(line.unwrap().as_str()).unwrap());
				}
			}
			return titleres;
		},
		Some(mut titleres) => {
			titleres.clear();
			for line in titleresfile.lines() {
				if line.is_ok() {
					titleres.push(Regex::new(line.unwrap().as_str()).unwrap());
				}
			}
			return titleres;
		},
	};
}

fn load_descres(exists: Option<Vec<Regex>>) -> Vec<Regex> {
	let descresf = OpenOptions::new().read(true).write(true).create(true).open("/home/bob/etc/snbot/descres.txt");
	if descresf.is_err() {
		println!("Error opening descres.txt: {:?}", descresf);
		let wtf: Vec<Regex> = Vec::new();
		return wtf;
	}
	let damnit = &descresf.unwrap();
	let descresfile = BufReader::new(damnit);

	match exists {
		None => {
			let mut descres: Vec<Regex> = Vec::new();
			for line in descresfile.lines() {
				if line.is_ok() {
					descres.push(Regex::new(line.unwrap().as_str()).unwrap());
				}
			}
			return descres;
		},
		Some(mut descres) => {
			descres.clear();
			for line in descresfile.lines() {
				if line.is_ok() {
					descres.push(Regex::new(line.unwrap().as_str()).unwrap());
				}
			}
			return descres;
		},
	};
}

fn load_channels(conn: &Connection) -> Vec<MyChannel> {
	let mut channels: Vec<MyChannel> = Vec::new();
	let mut stmt = conn.prepare("SELECT * FROM channels").unwrap();
	let mut allrows = stmt.query_map(&[], |row| {
		let iprotected: i32 = row.get(1);
		let mut protected: bool = false;
		if iprotected != 0 { protected = true; }
		MyChannel {
			name: row.get(0),
			protected: protected,
		}
	}).unwrap();
	for channel in allrows {
		if channel.is_ok() {
			channels.push(channel.unwrap());
		}
	}
	return channels;
}

fn get_youtube(go_key: &String, query: &String) -> String {
	let mut dst = Vec::new();
	let mut easy = Easy::new();
	let querybytes = query.clone().into_bytes();
	let encquery = easy.url_encode(&querybytes[..]);
	let url = format!("https://www.googleapis.com/youtube/v3/search/?maxResults=1&q={}&order=relevance&type=video&part=snippet&key={}", encquery, go_key);
	easy.url(url.as_str()).unwrap();
	easy.fail_on_error(true);
	{
		let mut transfer = easy.transfer();
		body_only(transfer, &mut dst);
	}

	if easy.response_code().unwrap_or(999) != 200 {
		println!("got http response code {}", easy.response_code().unwrap_or(999));
		return "Something borked, check the logs.".to_string();
	}
	let json = str::from_utf8(&dst[..]).unwrap_or("");
	let jsonthing = Json::from_str(json).unwrap_or(Json::from_str("{}").unwrap());
	let resopt = jsonthing.find_path(&["items"]);
	if resopt.is_none() {
		return format!("Got bad response from youtube API");
	}
	let resopt = resopt.unwrap();
	let cleanme = resopt[0].find_path(&["id", "videoId"]).unwrap().as_string().unwrap().to_string();
	return cleanme;

}

fn send_submission(submission: &Submission) -> bool {
	let url: String;
	if DEBUG {
		url = "https://dev.soylentnews.org/api.pl?m=story&op=post".to_string().trim().to_string();
	}
	else {
		url = "https://soylentnews.org/api.pl?m=story&op=post".to_string().trim().to_string();
	}
	let fooclone;
	let mut postdata = "foo".as_bytes();
	let mut dst = Vec::new();
	let mut easy = Easy::new();
	let subjectbytes = submission.subject.clone().into_bytes();
	let encsubject = easy.url_encode(&subjectbytes[..]);
	let storybytes = submission.story.clone().into_bytes();
	let encstory = easy.url_encode(&storybytes[..]);
	if DEBUG {
		let foo = format!("primaryskid=1&sub_type=plain&tid=10&name=MrPlow&reskey={}&subj={}&story={}", submission.reskey, encsubject, encstory);
		println!("{}", foo);
		fooclone = foo.clone();
		postdata = fooclone.as_bytes();
	}
	else {
		let foo = format!("primaryskid=1&sub_type=plain&tid=10&name={}&reskey={}&subj={}&story={}", submission.botnick, submission.reskey, encsubject, encstory);
		fooclone = foo.clone();
		postdata = fooclone.as_bytes();
	}

	easy.url(url.as_str()).unwrap();
	easy.cookie(submission.cookie.as_str()).unwrap();
	easy.post_field_size(postdata.len() as u64).unwrap();
	easy.post_fields_copy(postdata).unwrap();
	easy.post(true).unwrap();
	easy.fail_on_error(true);
	{
		let mut transfer = easy.transfer();
		body_only(transfer, &mut dst);
	}
	if easy.response_code().unwrap_or(999) != 200 {
		println!("got http response code {} for send_submission", easy.response_code().unwrap_or(999));
		return false;
	}
	let output: String = String::from_utf8(dst).unwrap_or("".to_string());
	if DEBUG {
		println!("{}", output);
	}
	return true;
}

fn get_bing_token(botconfig: &BotConfig) -> String {
	let url = "https://datamarket.accesscontrol.windows.net/v2/OAuth2-13/";
	let mut postdata = "foo".as_bytes();
	let mut dst = Vec::new();
	let mut easy = Easy::new();
	let secretbytes = &botconfig.bi_key[..].as_bytes();
	let postfields = format!("grant_type=client_credentials&scope=http://api.microsofttranslator.com&client_id=TMBuzzard_Translator&client_secret={}", easy.url_encode(secretbytes));
	let postbytes = postfields.as_bytes();
	easy.url(url).unwrap();
	easy.post_field_size(postbytes.len() as u64).unwrap();
	easy.post_fields_copy(postbytes).unwrap();
	easy.post(true).unwrap();
	{
		let mut transfer = easy.transfer();
		body_only(transfer, &mut dst);
	}
	if easy.response_code().unwrap_or(999) != 200 {
		println!("got http response code {} for get_bing_token", easy.response_code().unwrap_or(999));
		return "".to_string();
	}
	let json: String = String::from_utf8(dst).unwrap_or("".to_string());
	let jsonthing = Json::from_str(json.as_str()).unwrap_or(Json::from_str("{}").unwrap());
	let tokenopt = jsonthing.find("access_token");
	let mut token: String;
	if tokenopt.is_some() {
		token = tokenopt.unwrap().to_string().trim().to_string();
		if &token[0..1] == "\"" {
			let ctoken = token.clone();
			let strtoken = ctoken.as_str();
			let len = strtoken.len() - 1;
			token = strtoken[1..len].to_string().trim().to_string();
		}
	}
	else {
		token = "".to_string();
	}
	return token;
}

fn is_ns_faker(server: &IrcServer, nick: &String) -> bool {
	return false;
}

fn is_nick_here(server: &IrcServer, chan: &String, nick: &String) -> bool {
	let nicklist = server.list_users(&chan.as_str());
	if nicklist.is_none() {
		println!("got NONE for server.list_users('{}')", &chan);
		return false;
	}
	for user in nicklist.unwrap() {
		if &user.get_nickname() == &nick.as_str() {
			return true;
		}
	}
	return false;
}

fn get_raw_feed(feed: &String) -> String {
	let mut dst = Vec::new();
	let mut easy = Easy::new();
	let url = feed.clone();
	easy.url(url.as_str()).unwrap();
	easy.fail_on_error(true);
	{
		let mut transfer = easy.transfer();
		body_only(transfer, &mut dst);
	}
	if easy.response_code().unwrap_or(999) != 200 {
		println!("got http response code {}", easy.response_code().unwrap_or(999));
		return "Something borked, check the logs.".to_string();
	}
	let feed_data = str::from_utf8(&dst[..]).unwrap_or("");
	return feed_data.to_string();
}

fn get_feed_title(feed: &String) -> String {
	let feedstr = feed.as_str();
	if is_atom(&feedstr) {
		let parsed = feedstr.parse::<Feed>().unwrap();
		return parsed.title.to_string();
	}
	else if is_rss2(&feedstr) {
		let parsed = feedstr.parse::<Rss>().unwrap();
		return parsed.0.title.to_string();
	}
	else {
		return "Unknown feed type".to_string();
	}
}

fn is_atom(feedstr: &str) -> bool {
	let re = Regex::new(r"xmlns=\S+Atom").unwrap();
	if re.is_match(feedstr) {
		return true;
	}
	else {
		return false;
	}
}

fn is_rss2(feedstr: &str) -> bool {
	let re = Regex::new(r"<rss.*?version=.2\.0.").unwrap();
	if re.is_match(feedstr) {
		return true;
	}
	else {
		return false;
	}
}
// Returns the number of ms until next recurrence if this is a recurring timer
fn handle_timer(server: &IrcServer, feedbacktx: &Sender<Timer>, conn: &Connection, timer: &TimerTypes) -> u64 {
	match timer {
		&TimerTypes::Action { ref chan, ref msg } => { server.send_action(&chan, &msg); return 0_u64; },
		&TimerTypes::Message { ref chan, ref msg } => { server.send_privmsg(&chan, &msg); return 0_u64; },
		&TimerTypes::Once { ref command } => {
			match &command[..] {
				"goodfairy" => {
					let chan = "#fite".to_string();
					command_goodfairy( &server, &conn, &chan );
				},
				_ => {},
			};
			return 0_u64;
		},
		&TimerTypes::Recurring { ref every, ref command } => {
			match &command[..] {
				"goodfairy" => { 
					let chan = "#fite".to_string();
					command_goodfairy( &server, &conn, &chan );
				},
				"scoreboard" => {
					let chan = "#fite".to_string();
					fitectl_scoreboard(&server, &conn, &chan, false);
				},
				_ => {},
			};
			return every.clone() as u64;
		},
		&TimerTypes::Sendping { ref doping } => {
			let timer = Timer {
				delay: 0,
				action: TimerTypes::Feedback{
					command: "fiteoff".to_string(),
				},
			};
			// send msg to turn off botconfig.is_fighting
			feedbacktx.send(timer);
			// send server ping to get us a response that will trigger a read of feedbackrx
			server.send(Message{tags: None, prefix: None, command: Command::PING("irc.soylentnews.org".to_string(), None)});
			return 0_u64;
		},
		_ => {return 0_u64;},
	};
}

fn get_recurring_timers(conn: &Connection) -> Vec<TimerTypes> {
	let mut recurringTimers: Vec<TimerTypes> = Vec::new();
	let mut stmt = conn.prepare("SELECT * FROM recurring_timers").unwrap();
	let mut allrows = stmt.query_map(&[], |row| {
		TimerTypes::Recurring {
			every: row.get(0),
			command: row.get(1),
		}
	}).unwrap();
	for timer in allrows {
		if timer.is_ok() {
			recurringTimers.push(timer.unwrap());
		}
	}
	let recurringTimers = recurringTimers;
	return recurringTimers;
}

// Begin fite code
fn fite(server: &IrcServer, timertx: &Sender<Timer>, conn: &Connection, botconfig: &BotConfig, chan: &String, attacker: &String, target: &String) -> bool {
	let spamChan = "#fite".to_string();
	let mut msgDelay = 0_u64;
	let msg = format!("{}fite spam going to channel {}", &botconfig.prefix, &spamChan);
	server.send_privmsg(&chan, &msg);
	let mut oAttacker: Character = get_character(&conn, &attacker);
	let mut oDefender: Character = get_character(&conn, &target);
	let mut rng = rand::thread_rng();
	let mut rAttacker: &mut Character;
	let mut rDefender: &mut Character;
	let mut surprise: bool = false;
	let mut aFumble: bool = false;
	let mut dFumble: bool = false;

	// Make sure both characters are currently alive
	if !is_alive(&oAttacker) {
		let err = format!("#fite How can you fight when you're dead? Try again tomorrow.");
		server.send_privmsg(&chan, &err);
		return false;
	}
	if !is_alive(&oDefender) {
		let err = format!("#fite {}'s corpse is currently rotting on the ground. Try fighting someone who's alive.", &target);
		server.send_privmsg(&chan, &err);
		return false;
	}

	// Roll initiative
	oAttacker.initiative = roll_once(10_u8);
	oDefender.initiative = roll_once(10_u8);
	// No ties
	while oAttacker.initiative == oDefender.initiative {
		oAttacker.initiative = roll_once(10_u8);
		oDefender.initiative = roll_once(10_u8);
	}

	// Decide who goes first
	if oAttacker.initiative > oDefender.initiative {
		rAttacker = &mut oAttacker;
		rDefender = &mut oDefender;
		if roll_once(2_u8) == 2 {
			surprise = true;
			let msg = format!("{} sneaks up and ambushes {}", &rAttacker.nick, &rDefender.nick);
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			timertx.send(sendme);
			msgDelay += 1000_u64;
		}
	}
	else {
		rDefender = &mut oAttacker;
		rAttacker = &mut oDefender;
	}

	let vbold = vec![2];
	let vitallic = vec![29];
	let vclearall = vec![15];
	let vcolor = vec![3];
	let bold = str::from_utf8(&vbold).unwrap();
	let itallic = str::from_utf8(&vitallic).unwrap();
	let color = str::from_utf8(&vcolor).unwrap();
	let clearall = str::from_utf8(&vclearall).unwrap();
	let anick = format!("{}{}{}", &bold, &rAttacker.nick, &clearall);
	let dnick = format!("{}{}{}", &bold, &rDefender.nick, &clearall);
	let aweapon = format!("{}{}{}", &itallic, &rAttacker.weapon, &clearall);
	let dweapon = format!("{}{}{}", &itallic, &rDefender.weapon, &clearall);
	let aarmor = format!("{}{}{}", &itallic, &rAttacker.armor, &clearall);
	let darmor = format!("{}{}{}", &itallic, &rDefender.armor, &clearall);

	// Do combat rounds until someone dies
	loop {
		// whoever won init's turn
		let mut attackRoll: u8 = roll_once(20_u8);
		let mut damageRoll: u8 = 0;
		// Previously Fumbled
		if aFumble {
			aFumble = false;
			let msg = format!("{}{} retrieves their {} from the ground", &clearall, &anick, &aweapon);
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			timertx.send(sendme);
		}
		// Crit
		else if attackRoll == 20_u8 {
			damageRoll = roll_dmg() * 2;
			let msg = format!("{}{} smites the everlovin crap out of {} with a {} ({}04{}{})", &clearall, &anick, &dnick, &aweapon, &color, damageRoll, &color);
			if damageRoll as u64 > rDefender.hp {
				damageRoll = rDefender.hp as u8;
			}
			rDefender.hp = rDefender.hp - (damageRoll as u64);
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			timertx.send(sendme);
		}
		// Hit
		else if attackRoll > ARMOR_CLASS {
			damageRoll = roll_dmg();
			let msg = format!("{}{} clobbers {} upside their head with a {} ({}14{}{})", &clearall, &anick, &dnick, &aweapon, &color, damageRoll, &color);
			if damageRoll as u64 > rDefender.hp {
				damageRoll = rDefender.hp as u8;
			}
			rDefender.hp = rDefender.hp - (damageRoll as u64);
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			timertx.send(sendme);
		}
		// Fumble
		else if attackRoll == 1_u8 {
			aFumble = true;
			let msg = format!("{}{}'s {} slips from their greasy fingers", &clearall, &anick, &aweapon);
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			timertx.send(sendme);
		}
		// Miss
		else {
			let msg = format!("{}{} swings mightily but their {} is deflected by {}'s {}", &clearall, &anick, &aweapon, &dnick, &darmor);
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			timertx.send(sendme);
		}
		// Bail if rDefender is dead
		if !is_alive(&rDefender) {
			rAttacker.level = rAttacker.level + 1;
			rAttacker.hp = rAttacker.hp + 1;
			let deathRoll = roll_once(2_u8);
			if rDefender.level > 1 && (rAttacker.level > 15 || deathRoll == 1) {
				rDefender.level = rDefender.level - 1;
			}
			let deathmsg = format!("#fite {} falls broken at {}'s feet.", &dnick, &anick);
			let sendme: Timer = Timer {
				delay: msgDelay + 1000_u64,
				action: TimerTypes::Message{
						chan: chan.clone(),
						msg: deathmsg,
				},
			};
			timertx.send(sendme);
			break;
		}
		msgDelay += 1000_u64;
		if surprise {
			surprise = false;
			continue;
		}
		// whoever lost init's turn
		attackRoll = roll_once(20_u8);
		// Previously Fumbled
		if dFumble {
			dFumble = false;
			let msg = format!("{}{} retrieves their {} from the ground", &clearall, &dnick, &dweapon);
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			timertx.send(sendme);
		}
		// Crit
		else if attackRoll == 20_u8 {
			damageRoll = roll_dmg() * 2;
			let msg = format!("{}{} smites the everlovin crap out of {} with a {} ({}04{}{})", &clearall, &dnick, &anick, &dweapon, &color, damageRoll, &color);
			if damageRoll as u64 > rAttacker.hp {
				damageRoll = rAttacker.hp as u8;
			}
			rAttacker.hp = rAttacker.hp - (damageRoll as u64);
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			timertx.send(sendme);
		}
		// Hit
		else if attackRoll > ARMOR_CLASS {
			damageRoll = roll_dmg();
			let msg = format!("{}{} clobbers {} upside their head with a {} ({}14{}{})", &clearall, &dnick, &anick, &dweapon, &color, damageRoll, &color);
			if damageRoll as u64 > rAttacker.hp {
				damageRoll = rAttacker.hp as u8;
			}
			rAttacker.hp = rAttacker.hp - (damageRoll as u64);
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			timertx.send(sendme);
		}
		// Fumble
		else if attackRoll == 1_u8 {
			dFumble = true;
			let msg = format!("{}{}'s {} slips from their greasy fingers", &clearall, &dnick, &dweapon);
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			timertx.send(sendme);
		}
		// Miss
		else {
			let msg = format!("{}{} swings mightily but their {} is deflected by {}'s {}.", &clearall, &dnick, &dweapon, &anick, &aarmor);
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			timertx.send(sendme);
		}
		// Bail if rAttacker is dead
		if !is_alive(&rAttacker) {
			rDefender.level = rDefender.level + 1;
			rDefender.hp = rDefender.hp + 1;
			let deathRoll = roll_once(2_u8);
			if rAttacker.level > 1 && (rAttacker.level > 15 || deathRoll == 1) {
				rAttacker.level = rAttacker.level - 1;
			}
			let deathmsg = format!("#fite {} falls broken at {}'s feet.", &anick, &dnick);
			let sendme: Timer = Timer {
				delay: msgDelay + 1000_u64,
				action: TimerTypes::Message{
						chan: chan.clone(),
						msg: deathmsg,
				},
			};
			timertx.send(sendme);
			break;
		}
		msgDelay += 1000_u64;
	}
	
	// Save characters
	save_character(&conn, &rAttacker);
	save_character(&conn, &rDefender);
	// Send a timer to the timer handling thread with msgDelay + 100 delay so it fires just after the last
	let timer = Timer {
		delay: msgDelay + 1100_u64,
		action: TimerTypes::Sendping {
			doping: true,
		},
	};
	timertx.send(timer);
	return true;
}

fn save_character(conn: &Connection, character: &Character) {
	let time: i64 = time::now_utc().to_timespec().sec;
	let level = character.level as i64;
	let hp = character.hp as i64;
	conn.execute("UPDATE characters SET level = ?, hp = ?, ts = ? WHERE nick = ?", &[&level, &hp, &time, &character.nick.as_str()]).unwrap();
}

fn roll_once(sides: u8) -> u8 {
	let mut rng = rand::thread_rng();
	let random = rng.gen::<u64>();
	let roll = ((random % (sides as u64)) + 1) as u8;
	return roll;
}

fn roll_dmg() -> u8 {
	let mut roll = roll_once(8_u8);
	let mut total = 0_u8;
	total += roll;
	while roll == 8 {
		roll = roll_once(8_u8);
		total += roll;
	}
	let total = total;
	return total;
}

fn character_exists(conn: &Connection, nick: &String) -> bool {
	let count: i32 = conn.query_row("SELECT count(nick) FROM characters WHERE nick = ?", &[&nick.as_str()], |row| {
		row.get(0)
	}).unwrap();
	if count == 1 {
		return true;
	}
	else {
		return false;
	}
}

fn create_character(conn: &Connection, nick: &String) {
	let time: i64 = time::now_utc().to_timespec().sec;
	conn.execute("INSERT INTO characters VALUES(?, 1, 10, 'fist', 'grungy t-shirt', ?)", &[&nick.as_str(), &time]).unwrap();
	return;
}

fn is_alive(character: &Character) -> bool {
	if character.hp > 0 {
		return true;
	}
	else {
		return false;
	}
}

fn get_character(conn: &Connection, nick: &String) -> Character {
	let (leveli, hpi, weapon, armor, tsi) = conn.query_row("SELECT * FROM characters WHERE nick = ?", &[&nick.as_str()], |row| {
		(
			row.get(1),
			row.get(2),
			row.get(3),
			row.get(4),
			row.get(5),
		)
	}).unwrap_or((0_i64, 0_i64, "".to_string(), "".to_string(), 0_i64));
	let mut character: Character = Character {
			nick: nick.clone(),
			level: leveli as u64,
			hp: hpi as u64,
			weapon: weapon,
			armor: armor,
			ts: tsi as u64,
			initiative: 0_u8,
	};
	return character;
}

fn fitectl_scoreboard(server: &IrcServer, conn: &Connection, chan: &String, quiet: bool) {
	let spamChan = "#fite".to_string();
	struct Row {
		nick: String,
		lvl: i32,
		hp: i32,
		w: String,
		a: String,
	}

	let mut stmt = conn.prepare("SELECT * FROM characters ORDER BY level DESC, hp DESC, nick").unwrap();
	let mut allrows = stmt.query_map(&[], |row| {
		Row {
			nick: row.get(0),
			lvl: row.get(1),
			hp: row.get(2),
			w: row.get(3),
			a: row.get(4),
		}
	}).unwrap();
	
	let mut f;
	match File::create("/srv/sylnt.us/fitescoreboard.html").map_err(|e| e.to_string()) {
		Ok(file) => {f = file;},
		Err(err) => { println!("{}", err); return; },
	}

	let mut outString: String = "<html><head><link rel='stylesheet' type='text/css' href='fite.css'><title>#fite Scoreboard</title></head><body><table>
<tr id='header'><td>Nick</td><td>Level</td><td>HitPoints</td><td>Weapon</td><td>Armor</td></tr>\n".to_string();

	for row in allrows {
		let mrow = row.unwrap();
		let headed;
		if mrow.hp == 0 {
			headed = " class='hedead'";
		}
		else {
			headed = "";
		}
		
		let msg = format!("<tr{}><td>{}</td><td class='no'>{}</td><td class='hp'>{}</td><td>{}</td><td>{}</td></tr>\n", headed, mrow.nick, mrow.lvl, mrow.hp, mrow.w, mrow.a);
		outString.push_str(&msg.as_str());
	}
	outString.push_str("</table></body></html>");

	let outData = outString.as_bytes();
	match f.write_all(outData) {
		Ok(_) => {
			let msg = format!("#fite scoreboard updated: https://sylnt.us/fitescoreboard.html");
			if !quiet {
				server.send_privmsg(&chan, &msg);
			}
		},
		Err(err) => { println!("{}", err); },
	};
	return;
}

fn fitectl_status(server: &IrcServer, conn: &Connection, chan: &String, nick: &String) {
	if !character_exists(&conn, &nick) {
		create_character(&conn, &nick);
	}
	
	struct Row {
		nick: String,
		lvl: i32,
		hp: i32,
		w: String,
		a: String,
	}
	let result: Row = conn.query_row("SELECT * FROM characters WHERE nick = ?", &[&nick.as_str()], |row| {
		Row {
			nick: row.get(0),
			lvl: row.get(1),
			hp: row.get(2),
			w: row.get(3),
			a: row.get(4),
		}
	}).unwrap();

	let msg = format!("#fite {} level: {}, hp: {}, weapon: '{}', armor: '{}'", result.nick, result.lvl, result.hp, result.w, result.a);
	server.send_privmsg(&chan, &msg);
}

fn fitectl_weapon(server: &IrcServer, conn: &Connection, chan: &String, nick: &String, weapon: String) {
	if !character_exists(&conn, &nick) {
		create_character(&conn, &nick);
	}
	conn.execute("UPDATE characters SET weapon = ? WHERE nick = ?", &[&weapon.as_str(), &nick.as_str()]).unwrap();
	let msg = format!("#fite weapon for {} set to {}.", &nick, &weapon);
	server.send_privmsg(&chan, &msg);
}

fn fitectl_armor(server: &IrcServer, conn: &Connection, chan: &String, nick: &String, armor: String) {
	if !character_exists(&conn, &nick) {
		create_character(&conn, &nick);
	}
	conn.execute("UPDATE characters SET armor = ? WHERE nick = ?", &[&armor.as_str(), &nick.as_str()]).unwrap();
	let msg = format!("#fite armor for {} set to {}.", &nick, &armor);
	server.send_privmsg(&chan, &msg);
}
