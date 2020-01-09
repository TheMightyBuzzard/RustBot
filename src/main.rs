#![allow(non_snake_case)]
extern crate curl;
extern crate irc;
extern crate rusqlite;
extern crate serde;
extern crate serde_json;
extern crate rustc_serialize;
extern crate regex;
extern crate time;
extern crate rand;
extern crate geocoding;
extern crate chrono;

#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate lazy_static;

use std::{env, thread, str};
use std::process::exit;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufRead, Write};
use std::sync::mpsc::{Sender, Receiver};
use std::sync::{mpsc, Mutex, Arc};
use std::time::Duration;
use regex::Regex;
use curl::easy::{Easy};
use irc::client::prelude::*;
use serde_json::{Value};
use rustc_serialize::json::Json;
use rusqlite::Connection;
use rand::Rng;
use geocoding::{Opencage, Forward, Point, Reverse};
use chrono::{TimeZone, FixedOffset, Local};

#[derive(Deserialize, Serialize, Debug, Clone)]
struct BotConfig {
	owners: Vec<String>,
	nick: String,
	altn1: String,
	altn2: String,
	server: String,
	port: u16,
	channels: Vec<String>,
	protected: Vec<String>,
	prefix: String,
	pass: String,
	wu_key: String,
	go_key: String,
	bi_key: String,
	oc_key: String,
	dw_key: String,
	cse_id: String,
}

impl BotConfig {
	pub fn new() -> BotConfig {
		BotConfig {
			owners: Vec::new(),
			nick: "".to_string(),
			altn1: "".to_string(),
			altn2: "".to_string(),
			server: "".to_string(),
			port: 0_u16,
			channels: Vec::new(),
			protected: Vec::new(),
			prefix: "".to_string(),
			pass: "".to_string(),
			wu_key: "".to_string(),
			go_key: "".to_string(),
			bi_key: "".to_string(),
			oc_key: "".to_string(),
			dw_key: "".to_string(),
			cse_id: "".to_string(),
		}
	}
	pub fn load(&mut self, new: BotConfig) {
		self.owners = new.owners;
		self.nick = new.nick;
		self.altn1 = new.altn1;
		self.altn2 = new.altn2;
		self.server = new.server;
		self.port = new.port;
		self.channels = new.channels;
		self.protected = new.protected;
		self.prefix = new.prefix;
		self.pass = new.pass;
		self.wu_key = new.wu_key;
		self.go_key = new.go_key;
		self.bi_key = new.bi_key;
		self.oc_key = new.oc_key;
		self.dw_key = new.dw_key;
		self.cse_id = new.cse_id;
	}
	// TODO: write a saving command
	/*
	pub fn save(self) {
		let bot_config = format!("/home/bob/etc/snbot/config.json");
		match File::create(&bot_config) {
			Ok(file) => {
				let _ = serde_json::to_writer_pretty(file, &self);
			},
			Err(err) => {
				println!("error creating BotConfig file: {:#?}", err);
			},
		};
	}
	*/
	pub fn destroy(self) {
		let _ = self;
	}
}

#[derive(Debug, Clone)]
struct BotState {
	cookie: String,
	is_fighting: bool,
	ns_waiting: bool,
}

impl BotState {
	pub fn new() -> BotState {
		BotState {
			cookie: "".to_string(),
			is_fighting: false,
			ns_waiting: false,
		}
	}
}

#[derive(Debug)]
struct CacheEntry {
	age: i64,
	location: String,
	weather: String,
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

#[derive(Debug, Clone)]
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
	Savechars { attacker: Character, defender: Character },
}

#[derive(Debug)]
struct Timer {
	delay: u64,
	action: TimerTypes,
}

#[derive(Debug)]
struct NSResponse {
	username: String,
	hostmask: String,
	nickname: String,
	nsname: String,
}

#[derive(Debug)]
struct MyCommand {
	snick: String,
	hostmask: String,
	chan: String,
	said: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct DayOfWeather {
	time: i64,
	summary: String,
	precipProbability: f32,
	temperatureHigh: f32,
	temperatureLow: f32,
	humidity: f32,
	pressure: f32,
	windSpeed: f32,
	windGust: f32,
	cloudCover: f32,
}

#[derive(Serialize, Deserialize, Debug)]
struct WeatherDaily {
	summary: String,
	icon: String,
	data: Vec<DayOfWeather>,
}

#[derive(Serialize, Deserialize, Debug)]
struct WeatherJSON {
	latitude: f32,
	longitude: f32,
	timezone: String,
	daily: WeatherDaily,
	offset: i32,
}

#[derive(Serialize, Deserialize, Debug)]
struct FiteEffect {
	id: &str,
	PrettyName: String,
	dmgbuffmod: u64,
	dmgbuff: u64,
	dmgdebuffmod: u64,
	dmgdebuff: u64,
	dotdmg: u64,
	acbuff: u8,
	acdebuff: u8,
	hitbuff: u8,
	hitdebuff: u8,
	healmod: u64,
	heal: u64,
	bonusattacks: u8,
	slowrounds: u8,
	duration: u8,
	summonid: &str,
	summonmin: u8,
	summonmax: u8,
	issummon: bool,
	starttext: &str,
	stoptext: &str,
}

const VERSION: &str = "0.3.0";
const SOURCE: &str = "https://github.com/TheMightyBuzzard/RustBot";
const DEBUG: bool = false;
const ARMOR_CLASS: u8 = 10;
const MAX_DL_SIZE: f64 = 104857600f64;

lazy_static! {
	static ref BOTCONFIG: Arc<Mutex<BotConfig>> = Arc::new(Mutex::new(BotConfig::new()));
	static ref BOTSTATE: Arc<Mutex<BotState>> = Arc::new(Mutex::new(BotState::new()));
	static ref CONN: Arc<Mutex<Connection>> = Arc::new(Mutex::new(Connection::open("/home/bob/etc/snbot/usersettings.db").unwrap()));
	static ref TITLERES: Arc<Mutex<Vec<Regex>>> = Arc::new(Mutex::new(vec![]));
	static ref DESCRES: Arc<Mutex<Vec<Regex>>> = Arc::new(Mutex::new(vec![]));
	static ref WUCACHE: Arc<Mutex<Vec<CacheEntry>>> = Arc::new(Mutex::new(vec![]));
	static ref FITEEFFECTS: Arc<Mutex<Vec<FiteEffect>>> = Arc::new(Mutex::new(vec![]));
}

fn main() {
	let args: Vec<_> = env::args().collect();
	if args.len() < 2 {
		println!("Syntax: rustbot botnick");
		exit(0);
	}
	let thisbot = args[1].clone();
	prime_weather_cache();
	load_titleres();
	load_descres();

	{
		let botconfig: BotConfig = get_bot_config(&thisbot);
		match BOTCONFIG.lock() {
			Err(err) => {
				println!("Error locking BOTCONFIG: {:#?}", err);
				exit(1);
			},
			Ok(mut cfg) => cfg.load(botconfig),
		};
	}

	// Create a temporary variable 
	let mut botconfig: BotConfig = BotConfig::new();
	{
		match BOTCONFIG.lock() {
			Err(err) => {
				println!("Error locking BOTCONFIG: {:#?}", err);
				exit(1);
			},
			Ok(cfg) => botconfig.load(cfg.clone()),
		};
	}

	// TODO get rid of storables
	let server = IrcServer::from_config(
		irc::client::data::config::Config {
			owners: Some(botconfig.owners.clone()),
			nickname: Some(botconfig.nick.clone()),
			alt_nicks: Some(vec!(botconfig.altn1.clone(), botconfig.altn2.clone())),
			username: Some(botconfig.nick.clone()),
			realname: Some(botconfig.nick.clone()),
			server: Some(botconfig.server.clone()),
			port: Some(6667),
			password: Some(botconfig.pass.clone()),
			use_ssl: Some(false),
			encoding: Some("UTF-8".to_string()),
			version: Some(VERSION.to_string()),
			source: Some(SOURCE.to_string()),
			channels: Some(botconfig.channels.clone()),
			channel_keys: None,
			umodes: Some("+Zix".to_string()),
			user_info: Some("MrPlow rewritten in Rust".to_string()),
			ping_time: Some(180),
			ping_timeout: Some(10),
			ghost_sequence: Some(vec!("RECOVER".to_string())),
			should_ghost: Some(true),
			nick_password: Some(botconfig.pass.clone()),
			options: None,
			burst_window_length: Some(8),
			max_messages_in_burst: Some(24),
			cert_path: None,
			use_mock_connection: Some(false),
			mock_initial_value: None
		}
	).unwrap();
	botconfig.destroy();


	let recurringTimers: Vec<TimerTypes> = get_recurring_timers();

	server.identify().unwrap();

	// Feedback channel that any thread can write to?
	let (feedbacktx, feedbackrx) = mpsc::channel::<Timer>();

	// Spin off a submitter listening thread
	let (subtx, subrx) = mpsc::channel::<Submission>();
	{	
		let server = server.clone();
		let _ = thread::spawn(move || {
			loop {
				for submission in subrx.recv() {
					if DEBUG {
						println!("{:#?}", submission);
					}
					thread::sleep(Duration::new(25,0));
					let chan = submission.chan.clone();
					if send_submission(&submission) {
						let _ = server.send_privmsg(&chan, "Submission successful. https://soylentnews.org/submit.pl?op=list");
					}
					else {
						let _ = server.send_privmsg(&chan, "Something borked during submitting, check the logs.");
					}
				}
			}
		});
	}
	
	// Spin off a timed event thread
	let (timertx, timerrx) = mpsc::channel::<Timer>();
	{
		let server = server.clone();
		let _ = thread::spawn(move || {
			let mut qTimers: Vec<Timer> = Vec::new();
			for timer in recurringTimers {
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
			let tenthSecond = Duration::from_millis(100);
			loop {
				match timerrx.try_recv() {
					Err(_) => { },
					Ok(timer) => {
						if DEBUG {
							println!("{:#?}", timer);
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
							timer.delay = handle_timer(&server, &feedbacktx, &timer.action);
						}
					}
					
					// Drop all timers we've already executed at once to save time
					qTimers.retain(|ref t| t.delay != 0_u64);
				}
				
				thread::sleep(tenthSecond);
			}
		});
	}

	// let's have us some async Message handling
	/*
	let (msgtx, msgrx) = mpsc::channel::<irc::proto::message::Message>();
	{	
		let server = server.clone();
		let _ = thread::spawn(move || {
			let _ = server.for_each_incoming(|message| {
				let umsg = message.clone();
				msgtx.send(umsg).unwrap();
			});
		});
	}
	*/

	let tGoodfairy = Timer {
		delay: 5000_u64,
		action: TimerTypes::Once {
			command: "goodfairy".to_string(),
		}
	};
	let _ = timertx.send(tGoodfairy);

	let (whotx, whorx) = mpsc::channel::<NSResponse>();

	// let's have us some threaded command handling
	let (cmdtx, cmdrx) = mpsc::channel::<MyCommand>();
	{
		let server = server.clone();
		let _ = thread::spawn(move || {
			loop {
				match cmdrx.try_recv() {
					Err(_) => {},
					Ok(command) => process_command(&server, &subtx, &timertx, &whorx, &command.snick, &command.hostmask, &command.chan, &command.said),
				};
				let tenthSecond = Duration::from_millis(100);
				thread::sleep(tenthSecond);
			}
		});
	}

	// main loop
	let _ = server.for_each_incoming(|message| {
		let umessage = message.clone();
		let nick = umessage.source_nickname();
		let snick: String;
		if DEBUG {
			println!("{:?}", umessage);
		}
		match umessage.command {
			irc::proto::command::Command::PRIVMSG(ref chan, ref untrimmed) => {
				let said = untrimmed.trim_end().to_string();
				let hostmask = umessage.prefix.clone().unwrap().to_string();
				snick = nick.unwrap().to_string();
				if check_messages(&snick) {
					deliver_messages(&server, &snick);
				}

	
				if is_action(&said) {
					let mut asaid = said.clone();
					asaid = asaid[8..].to_string();
					let asaidend = asaid.len() - 1;
					asaid = asaid[..asaidend].to_string();
					log_seen(&chan, &snick, &hostmask, &asaid, 1);
					process_action(&server, &snick, &chan, &said);
				}
				else if is_command(&said) {
					// process_command moved into its own thread
					//process_command(&server, &subtx, &timertx, &whorx, &snick, &hostmask, &chan, &said);
					let command = MyCommand {
						snick:snick.clone(),
						hostmask: hostmask.clone(),
						chan: chan.clone(),
						said: said.clone(),
					};
					let _ = cmdtx.send(command);
					log_seen(&chan, &snick, &hostmask, &said, 0);
				}
				else {
					log_seen(&chan, &snick, &hostmask, &said, 0);
				}
			},
			irc::proto::command::Command::PING(_,_) => {},
			irc::proto::command::Command::PONG(_,_) => {
				match feedbackrx.try_recv() {
					Err(_) => { },
					Ok(timer) => {
						if DEBUG {
							println!("{:#?}", timer);
						}
						match timer.action {
							TimerTypes::Feedback {ref command} => {
								match &command[..] {
									"fiteoff" => {
										match BOTSTATE.lock() {
											Err(err) => println!("Error locking BOTSTATE: {:#?}", err),
											Ok(mut botstate) => botstate.is_fighting = false,
										};
									},
									_ => {},
								};
							},
							_ => {},
						};
						//qTimers.push(timer);
					}	
				};
			},
			irc::proto::command::Command::Response(ref code, ref argsvec, _) => {
				match *code {
					irc::proto::response::Response::RPL_WHOSPCRPL => {
						let nsresponse = NSResponse {
							username: "".to_string(),
							hostmask: "".to_string(),
							nickname: argsvec[1].to_string(),
							nsname: argsvec[2].to_string(),
						};
						println!("nsresponse: {:#?}", &nsresponse);
						let _ = whotx.send(nsresponse);
					},
					_ => {},
				};
			},
			_ => {},
		};
	});
}

fn get_bot_config(botnick: &String) -> BotConfig {
	let bot_config = format!("/home/bob/etc/snbot/config.json");
	match File::open(&bot_config) {
		Ok(file) => {
			match serde_json::from_reader(file) {
				Ok(configs) => {
					let allconfigs: Value = configs;
					match serde_json::from_value(allconfigs[&botnick[..]].clone()) {
						Ok(config) => return config,
						Err(err) => {
							println!("{:#?}", err);
							exit(1);
						},
					};
				},
				Err(err) => {
					println!("Could not read from {}: {:#?}", &bot_config, err);
					exit(1);
				},
			};
		},
		Err(err) => {
			println!("Could not open {}: {:#?}", &bot_config, err);
			exit(1);
		},
	};
}

fn get_fite_effects() -> Vec<FiteEffect> {
	let effects_file = format!("/home/bob/etc/snbot/fiteeffects.json");
	match File::open(&effects_file) {
		Ok(file) => {
			let alleffects: Vec<FiteEffect> = 
		},
		Err(err) => {
			println!("Could not read from {}: {:#?}", &effects_file, err);
			exit(1);
		}
	}
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

fn is_command(said: &String) -> bool {
	let mut prefix = "💩💩💩".to_string();
	match BOTCONFIG.lock() {
		Err(err) => println!("Could not lock BOTCONFIG: {:#?}", err),
		Ok(botconfig) => prefix = botconfig.prefix.clone(),
	};
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

fn log_seen(chan: &String, snick: &String, hostmask: &String, said: &String, action: i32) {
	match CONN.lock() {
		Err(err) => println!("Could not lock CONN: {:#?}", err),
		Ok(conn) => {
			let time: i64 = time::now_utc().to_timespec().sec;
			conn.execute("REPLACE INTO seen VALUES($1, $2, $3, $4, $5, $6)", &[snick, hostmask, chan, said, &time, &action]).unwrap();
		},
	};
}

fn process_action(server: &IrcServer, nick: &String, channel: &String, said: &String) {
	let prefix = "\u{1}ACTION ".to_string();
	let prefixlen = prefix.len();
	let end = said.len() - 1;
	let csaid: String = said.clone();
	let action: String = csaid[prefixlen..end].to_string();
	if action == "yawns" {
		let flip = format!("flips a Skittle into {}'s gaping mouth", nick);
		let _ = server.send_action( channel, &flip );
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

fn process_command(server: &IrcServer, subtx: &Sender<Submission>, timertx: &Sender<Timer>, whorx: &Receiver<NSResponse>, nick: &String, hostmask: &String, chan: &String, said: &String) {
	let prefix: String;
	{
		match BOTCONFIG.lock() {
			Err(err) => {
				println!("Could not lock BOTCONFIG: {:#?}", err);
				return;
			},
			Ok(botconfig) => {
				prefix = botconfig.prefix.clone();
			},
		};
	}
	let maskonly = hostmask_only(&hostmask);
	let prefixlen = prefix.len();
	let saidlen = said.len();
	let csaid: String = said.clone();
	let noprefix: String = csaid[prefixlen..saidlen].to_string().trim().to_string();
	let noprefixbytes = noprefix.as_bytes();
	if cmd_check(&noprefixbytes, "quit", true) {
		if !is_admin(&nick) {
			return;
		}
		command_quit(server, chan.to_string());
		return;
	}
	else if cmd_check(&noprefixbytes, "pissoff", true) {
		if !is_admin(&nick) {
			return;
		}
		command_pissoff(server, chan.to_string());
		return;
	}
	else if cmd_check(&noprefixbytes, "dieinafire", true) {
		if !is_admin(&nick) {
			return;
		}
		command_dieinafire(server, chan.to_string());
		return;
	}
	else if cmd_check(&noprefixbytes, "join", true) || cmd_check(&noprefixbytes, "join #", false) {
		if is_abuser(&server, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "join" {
			command_help(&server, &chan, Some("join".to_string()));
			return;
		}
		let joinchan = noprefix["join ".len()..].trim().to_string();
		command_join(&server, joinchan);
		return;
	}
	else if cmd_check(&noprefixbytes, "seen", true) || cmd_check(&noprefixbytes, "seen ", false) {
		if noprefix.as_str() == "seen" {
			command_help(&server, &chan, Some("seen".to_string()));
			return;
		}
		let who = noprefix["seen ".len()..].trim().to_string();
		command_seen(&server, &chan, who);
		return;
	}
	else if cmd_check(&noprefixbytes, "smakeadd", true) || cmd_check(&noprefixbytes, "smakeadd ", false) {
		if is_abuser(&server, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "smakeadd" {
			command_help(&server, &chan, Some("smakeadd".to_string()));
			return;
		}
		let what = noprefix["smakeadd ".len()..].trim().to_string();
		command_smakeadd(&server, &chan, what);
		return;
	}
	else if cmd_check(&noprefixbytes, "smake", true) || cmd_check(&noprefixbytes, "smake ", false) {
		if noprefix.as_str() == "smake" {
			command_help(&server, &chan, Some("smake".to_string()));
			return;
		}
		let who = noprefix["smake ".len()..].trim().to_string();
		command_smake(&server, &chan, who);
		return;
	}
	else if cmd_check(&noprefixbytes, "weatheradd", true) || cmd_check(&noprefixbytes, "weatheradd ", false) {
		if noprefix.len() < "weatheradd 12345".len() {
			command_help(&server, &chan, Some("weatheradd".to_string()));
			return;
		}
		let checklocation = noprefix["weatheradd ".len()..].trim().to_string();
		command_weatheradd(&server, &nick, &chan, checklocation);
		return;
	}
	else if cmd_check(&noprefixbytes, "weather", true) || cmd_check(&noprefixbytes, "weather ", false) {
		if is_abuser(&server, &chan, &maskonly) {
			return;
		}
		let checklocation: Option<String>;
		if noprefix.as_str() == "weather" {
			checklocation = None;
		}
		else {
			checklocation = Some(noprefix["weather ".len()..].trim().to_string());
		}
		command_weather(&server, &nick, &chan, checklocation);
		return;
	}
	else if cmd_check(&noprefixbytes, "abuser", true) || cmd_check(&noprefixbytes, "abuser ", false) {
		if !is_admin(&nick) {
			return;
		}
		if noprefix.as_str() == "abuser" {
			command_help(&server, &chan, Some("abuser".to_string()));
			return;
		}
		let abuser = noprefix["abuser ".len()..].trim().to_string();
		command_abuser(&server, &chan, abuser);
		return;
	}
	else if cmd_check(&noprefixbytes, "bot", true) || cmd_check(&noprefixbytes, "bot ", false) {
		if !is_admin(&nick) {
			return;
		}
		if noprefix.as_str() == "bot" {
			command_help(&server, &chan, Some("bot".to_string()));
			return;
		}
		let bot = noprefix["bot ".len()..].trim().to_string();
		command_bot(&server, &chan, bot);
		return;
	}
	else if cmd_check(&noprefixbytes, "admin", true) || cmd_check(&noprefixbytes, "admin ", false) {
		if !is_admin(&nick) {
			return;
		}
		// TODO write to current BotConfig then save it
		/*
		if noprefix.as_str() == "admin" {
			command_help(&server, &chan, Some("admin".to_string()));
			return;
		}
		let admin = noprefix["admin ".len()..].trim().to_string();
		command_admin(&server, &chan, admin);*/
		return;
	}
	else if cmd_check(&noprefixbytes, "submit", true) || cmd_check(&noprefixbytes, "submit ", false) {
		if is_abuser(&server, &chan, &maskonly) {
			return;
		}
		if noprefix.find("http").is_none() {
			command_help(&server, &chan, Some("submit".to_string()));
			return;
		}
		let (suburl, summary) = sub_parse_line(&noprefix);
		command_submit(&server, &chan, &subtx, suburl, summary, &nick);
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
		command_help(&server, &chan, command);
		return;
	}
	else if cmd_check(&noprefixbytes, "youtube", true) || cmd_check(&noprefixbytes, "youtube ", false) {
		if is_abuser(&server, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "youtube" {
			command_help(&server, &chan, Some("youtube".to_string()));
			return;
		}
		let query: String = noprefix["youtube ".len()..].trim().to_string();
		command_youtube(&server, &chan, query);
		return;
	}
	else if cmd_check(&noprefixbytes, "yt", true) || cmd_check(&noprefixbytes, "yt ", false) {
		if is_abuser(&server, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "yt" {
			command_help(&server, &chan, Some("youtube".to_string()));
			return;
		}
		let query: String = noprefix["yt ".len()..].trim().to_string();
		command_youtube(&server, &chan, query);
		return;
	}
	else if cmd_check(&noprefixbytes, "socialist", true) || cmd_check(&noprefixbytes, "socialist ", false) {
		if noprefix.as_str() == "socialist" {
			command_help(&server, &chan, Some("socialist".to_string()));
			return;
		}
		let _ = server.send_privmsg(&chan, format!("{}, you're a socialist!", &noprefix["socialist ".len()..].trim()).as_str());
		return;
	}
	else if cmd_check(&noprefixbytes, "roll", true) || cmd_check(&noprefixbytes, "roll ", false) {
		if noprefix.as_str() == "roll" {
			command_help(&server, &chan, Some("roll".to_string()));
			return;
		}
		let args = noprefix["roll ".len()..].trim().to_string();
		command_roll(&server, &chan, args);
		return;
	}
	else if cmd_check(&noprefixbytes, "bnk", true) {
		let _ = server.send_privmsg(&chan, "https://www.youtube.com/watch?v=pF_YEFJpHX8");
		return;
	}
	else if cmd_check(&noprefixbytes, "part", true) || cmd_check(&noprefixbytes, "part ", false) {
		if noprefix.as_str() == "part" {
			let partchan = chan.clone();
			command_part(&server, &chan, partchan);
		}
		else {
			let mut partchan = noprefix["part ".len()..].trim().to_string();
			let sp = partchan.find(" ");
			if sp.is_some() {
				let end = sp.unwrap();
				partchan = partchan[..end].trim().to_string();
			}
			command_part(&server, &chan, partchan);
		}
		return;
	}
	else if cmd_check(&noprefixbytes, "say", true) || cmd_check(&noprefixbytes, "say ", false) {
		if !is_admin(&nick) {
			return;
		}
		if noprefix.as_str() == "say" {
			command_help(&server, &chan, Some("say".to_string()));
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
			command_help(&server, &chan, Some("tell".to_string()));
			return;
		}
		let space = noprefix.find(" ").unwrap_or(0);
		if space == 0 { return; }
		let nocommand = noprefix[space..].trim().to_string();
		command_tell(&server, &chan, &nick, nocommand);
		return;
	}
	/*
	else if cmd_check(&noprefixbytes, "klingon", true) || cmd_check(&noprefixbytes, "klingon ", false) {
		if noprefix.as_str() == "klingon" {
			command_help(&server, &chan, Some("klingon".to_string()));
			return;
		}
		let english = noprefix["klingon ".len()..].trim().to_string();
		command_klingon(&server, &chan, english);
		return;
	}*/
	else if cmd_check(&noprefixbytes, "g", true) || cmd_check(&noprefixbytes, "g ", false) {
		if noprefix.as_str() == "g" {
			command_help(&server, &chan, Some("g".to_string()));
			return;
		}
		let searchstr = noprefix["g ".len()..].trim().to_string();
		command_google(&server, &chan, searchstr);
		return;
	}
	else if cmd_check(&noprefixbytes, "fite", true) || cmd_check(&noprefixbytes, "fite ", false) {
		match BOTSTATE.lock() {
			Err(err) => {
				println!("Could not lock BOTSTATE: {:#?}", err);
				return;
			},
			Ok(mut botstate) => {
				if noprefix.as_str() == "fite" {
					command_help(&server, &chan, Some("fite".to_string()));
					return;
				}
				if botstate.is_fighting {
					let msg = format!("There's already a fight going on. Wait your turn.");
					let _ = server.send_privmsg(&chan, &msg);
					return;
				}
				if &chan[..] != "#fite" {
					let _ = server.send_privmsg(&chan, "#fite restricted to the channel #fite");
					return;
				}
				// if the person asking isn't registered, register them
				if !is_nick_fiter(&nick) {
					if !is_nick_registered(&server, &whorx, &nick) {
						match register_fiter(&nick) {
							Err(err) => {
								println!("Could not register {} for fite: {:#?}", &nick, &err);
								return;
							},
							Ok(_) => {},
						};
					}
				}
				
				// start the fite
				botstate.is_fighting = true;
				let target = noprefix["fite ".len()..].trim().to_string();
				let stop = command_fite(&server, &timertx, &chan, &nick, target);
				if stop {
					fitectl_scoreboard(&server, true);
				}
				else {
					botstate.is_fighting = false;
				}
				return;
			},
		};
	}
	else if cmd_check(&noprefixbytes, "fitectl", true) || cmd_check(&noprefixbytes, "fitectl ", false) {
		if noprefix.as_str() == "fitectl" { 
			command_help(&server, &chan, Some("fitectl".to_string()));
			return;
		}
		let is_fighting;
		{
			match BOTSTATE.lock() {
				Err(err) => {
					println!("Could not lock BOTSTATE: {:#?}", err);
					return;
				},
				Ok(botstate) => is_fighting = botstate.is_fighting,
			};
		}
		if is_fighting {
			let msg = format!("There's a fight going on. You'll have to wait.");
			let _ = server.send_privmsg(&chan, &msg);
			return;
		}
		let args = noprefix["fitectl ".len()..].trim().to_string();
		command_fitectl(&server, &chan, &nick, args);
		return;
	}
	else if cmd_check(&noprefixbytes, "goodfairy", true) {
		if !is_admin(&nick) {
						return;
				}
		command_goodfairy(&server, &chan);
		return;
	}
	else if cmd_check(&noprefixbytes, "reloadregexes", true) {
		if !is_admin(&nick) {
			return;
		}
		load_titleres();
		load_descres();
		return;
	}
	else if cmd_check(&noprefixbytes, "sammichadd", true) || cmd_check(&noprefixbytes, "sammichadd ", false) {
		if is_abuser(&server, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "sammichadd" {
			command_help(&server, &chan, Some("sammichadd".to_string()));
			return;
		}
		let sammich = noprefix["sammichadd ".len()..].trim().to_string();
		command_sammichadd(&server, &chan, sammich);
		return;
	}
	else if cmd_check(&noprefixbytes, "sammich", true) || cmd_check(&noprefixbytes, "sammich ", false) {
		if noprefix.as_str() == "sammich" {
			command_sammich(&server, &chan, &nick);
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
	else if cmd_check(&noprefixbytes, "fakeweather", true) || cmd_check(&noprefixbytes, "fakeweather ", false) {
		if is_abuser(&server, &chan, &maskonly) {
						return;
				}
		if noprefix.as_str() == "fakeweather" {
			command_help(&server, &chan, Some("fakeweather".to_string()));
			return;
		}
				let what = noprefix["fakeweather ".len()..].trim().to_string();
				command_fake_weather_add(&server, &chan, what);
		return;
		}
	else if cmd_check(&noprefixbytes, "weatheralias", true) || cmd_check(&noprefixbytes, "weatheralias ", false) {
		if is_abuser(&server, &chan, &maskonly) {
			return;
		}
		if noprefix.as_str() == "weatheralias" {
					command_help(&server, &chan, Some("weatheralias".to_string()));
			return;
		}
		let what = noprefix["weatheralias ".len()..].trim().to_string();
		command_weather_alias(&server, &chan, what);
		return;
	}
	else if cmd_check(&noprefixbytes, "raw ", false) {
		if !is_admin(&nick) {
			return;
		}
		let what = noprefix["raw ".len()..].trim().to_string();
		do_raw(&server, &what[..]);
		return;
	}
}

fn do_raw(server: &IrcServer, data: &str) {
	let dome = irc::proto::command::Command::Raw(data.to_string().clone(), vec![], None);
	if !server.send(dome).is_ok() {
		println!("got some sort of error in processing a raw command");
	}
	return;
}

fn do_who(server: &IrcServer, who: &str) {
	let command = irc::proto::command::Command::Raw(format!("WHO {} %na", &who), vec!["%na".to_string()], Some(format!("%na")));
	if !server.send(command).is_ok() {
		println!("got some sort of error on do_who");
	}
	return;
}

fn command_fitectl(server: &IrcServer, chan: &String, nick: &String, args: String) {
	let argsbytes = args.as_bytes();
	if args.len() == 10 && &argsbytes[..] == "scoreboard".as_bytes() {
		fitectl_scoreboard(&server, false);
	}
	else if args.len() > 7 && &argsbytes[..6] == "armor ".as_bytes() {
		let armor = args[5..].trim().to_string();
		fitectl_armor(&server, &chan, &nick, armor);
	}
	else if args.len() > 8 && &argsbytes[..7] == "weapon ".as_bytes() {
		let weapon = args[7..].trim().to_string();
		fitectl_weapon(&server, &chan, &nick, weapon);
	}
	else if args.len() == 6 && &argsbytes[..6] == "status".as_bytes() {
		fitectl_status(&server, &chan, &nick);
	}
}

fn command_goodfairy(server: &IrcServer, chan: &String) {
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			conn.execute("UPDATE characters SET hp = level + 10", &[]).unwrap();
			let lucky: String = conn.query_row("SELECT nick FROM characters ORDER BY RANDOM() LIMIT 1", &[], |row| {
				row.get(0)
			}).unwrap();
			conn.execute("UPDATE characters SET hp = level + 100 WHERE nick = ?", &[&lucky]).unwrap();
			let _ = server.send_privmsg(&chan, "#fite The good fairy has come along and revived everyone");
			let _ = server.send_privmsg(&chan, format!("#fite the gods have smiled upon {}", &lucky).as_str() );
		},
	};
	fitectl_scoreboard(&server, true);
}

fn command_fite(server: &IrcServer, timertx: &Sender<Timer>, chan: &String, attacker: &String, target: String) -> bool {
	if is_nick_here(&server, &chan, &target) {
		if !sql_table_check("characters".to_string()) {
			println!("`characters` table not found, creating...");
			if !sql_table_create("characters".to_string()) {
				let _ = server.send_privmsg(&chan, "No characters table exists and for some reason I cannot create one");
				return false;
			}
		}
		if !character_exists(&attacker) {
			create_character(&attacker);
		}
		if !character_exists(&target) {
			create_character(&target);
		}

		let returnme = fite(&server, &timertx, &attacker, &target);
		return returnme;
	}
	else {
		let err = format!("#fite looks around but doesn't see {}", &target);
		let _ = server.send_action(&chan, &err);
		return false;
	}
}

fn command_sammichadd(server: &IrcServer, chan: &String, sammich: String) {
	if !sql_table_check("sammiches".to_string()) {
		println!("`sammiches` table not found, creating...");
		if !sql_table_create("sammiches".to_string()) {
			let _ = server.send_privmsg(&chan, "No sammiches table exists and for some reason I cannot create one");
			return;
		}
	}
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			match conn.execute("INSERT INTO sammiches VALUES(NULL, $1)", &[&sammich]) {
				Err(err) => {
					println!("{}", err);
					let _ = server.send_privmsg(&chan, "Error writing to sammiches table.");
					return;
				},
				Ok(_) => {
					let sayme: String = format!("\"{}\" added.", sammich);
					let _ = server.send_privmsg(&chan, &sayme);
					return;
				},
			};
		},
	};
}

fn command_sammich(server: &IrcServer, chan: &String, nick: &String) {
	if !sql_table_check("sammiches".to_string()) {
		println!("`sammiches` table not found, creating...");
		if !sql_table_create("sammiches".to_string()) {
			let _ = server.send_privmsg(&chan, "No sammiches table exists and for some reason I cannot create one");
			return;
		}
	}
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			let check: i32 = conn.query_row("select count(*) from sammiches", &[], |row| {
				row.get(0)
			}).unwrap();
			if check == 0 {
				let _ = server.send_privmsg(&chan, "No sammiches in the database, add some.");
			}

			let result: String = conn.query_row("select sammich from sammiches order by random() limit 1", &[], |row| {
				row.get(0)
			}).unwrap();

			let dome: String = format!("whips up a {} sammich for {}", result, nick);
			let _ = server.send_action(&chan, &dome);
		},
	};
}

fn command_sammich_alt(server: &IrcServer, chan: &String, target: &String) {
	if is_nick_here(&server, &chan, &target) {
		let sneak = format!("sneaks up behind {} and cuts their throat", &target);
		let makesammich = format!("fixes thinly sliced {}'s corpse sammiches for everyone in {}", &target, &chan);
		let _ = server.send_action(&chan, &sneak.as_str());
		let _ = server.send_action(&chan, &makesammich.as_str());
		return;
	}
	else {
		let action = format!("looks around but does not see {}", &target);
		let _ = server.send_action(&chan, &action);
		return;
	}
}

fn body_only<'a, 'b>(mut transfer: curl::easy::Transfer<'b, 'a>, dst: &'a mut Vec<u8>) {
	transfer.write_function(move |data: &[u8]| {
		dst.extend_from_slice(data);
		Ok(data.len())
	}).unwrap();
	transfer.progress_function(check_max_transfer_size).unwrap();
	let _ = transfer.perform();
}

fn headers_only<'a, 'b>(mut transfer: curl::easy::Transfer<'b, 'a>, dst: &'a mut Vec<u8>) {
	transfer.progress_function(check_max_transfer_size).unwrap();
	transfer.write_function(nullme).unwrap();
	let _ = transfer.header_function(move |data: &[u8]| {
		dst.extend_from_slice(data);
		true
	});
	let _ = transfer.perform();
}

fn check_max_transfer_size(expected: f64, current: f64, _: f64, _: f64) -> bool {
	let mut wegood: bool = true;
	if expected > MAX_DL_SIZE {
		wegood = false;
	}
	if current > MAX_DL_SIZE {
		wegood = false;
	}
	wegood
}

fn nullme(data: &[u8]) -> Result<usize,curl::easy::WriteError> {
	Ok(data.len())
}

fn command_google(server: &IrcServer, chan: &String, searchstr: String) {
	let go_key;
	let cse_id;
	let bkey;
	let bcx;
	match BOTCONFIG.lock() {
		Err(err) => {
			println!("Could not lock BOTCONFIG: {:#?}", err);
			return;
		},
		Ok(botconfig) => {
			go_key = botconfig.go_key.clone();
			bkey = go_key.into_bytes();
			cse_id = botconfig.cse_id.clone();
			bcx = cse_id.into_bytes();
		},
	};
	let mut dst = Vec::new();
	let mut easy = Easy::new();
	let bsearchstr = &searchstr.clone().into_bytes();
	let esearchstr = easy.url_encode(&bsearchstr[..]);
	let ecx = easy.url_encode(&bcx[..]);
	let ekey = easy.url_encode(&bkey[..]);
	let url = format!("https://www.googleapis.com/customsearch/v1?q={}&cx={}&safe=off&key={}", esearchstr, ecx, ekey);

	easy.url(url.as_str()).unwrap();
	easy.forbid_reuse(true).unwrap();
	easy.progress(true).unwrap();
	// Closure so that transfer will go poof after being used
	{
		let transfer = easy.transfer();
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
		let _ = server.send_privmsg(&chan, "sorry, there were no results for your query");
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
	let _ = server.send_privmsg(&chan, &response);
}

fn command_klingon(server: &IrcServer, chan: &String, english: String) {
	/*
	let token = get_bing_token();
	if token == "".to_string() {
		let _ = server.send_privmsg(&chan, "Could not get bing translate token, check the logs");
		return;
	}
	let outlangs = vec!["tlh", "tlh-Qaak"];
	let mut dst = Vec::new();
	let mut translations = Vec::new();
	for lang in outlangs.iter() {
		let mut headerlist = List::new();
		let _ = headerlist.append(format!("Authorization: Bearer {}", token).as_str());
		let _ = headerlist.append("Accept-Language: en-US");
		let _ = headerlist.append("Accept-Charset: utf-8");
		{
			let mut easy = Easy::new();
			let benglish = &english.clone().into_bytes();
			let eenglish = easy.url_encode(&benglish[..]);
			let url = format!("http://api.microsofttranslator.com/V2/Http.svc/Translate?text={}&from=en&to={}&contentType=text/plain", &eenglish, &lang);
			easy.url(url.as_str()).unwrap();
			easy.forbid_reuse(true).unwrap();
			easy.http_headers(headerlist).unwrap();
			{
				let transfer = easy.transfer();
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
	let _ = server.send_privmsg(&chan, format!("{} ({})	", tlh, qaak).as_str());
	*/
	return;
}

fn command_tell(server: &IrcServer, chan: &String, nick: &String, incoming: String) {
	let space = incoming.find(" ").unwrap_or(0);
	if space == 0 { return; }
	let tellwho = incoming[..space].trim().to_string();
	let tellwhat = incoming[space..].trim().to_string();
	if tellwho.len() < 1 || tellwhat.len() < 1 {
		return;
	}
	if save_msg(&nick, tellwho, tellwhat) {
		let _ = server.send_privmsg(&chan, "Okay, I'll tell them next time I see them.");
	}
	else {
		let _ = server.send_privmsg(&chan, "Something borked saving your message, check the logs.");
	}
	return;
}

fn command_roll(server: &IrcServer, chan: &String, args: String) {
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
		command_help(&server, &chan, Some("roll".to_string()));
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
		let _ = server.send_privmsg(&chan, format!("chromas, is that you? stop being a wiseass.").as_str());
		return;
	}
	else if side < 1 || dice < 1 || throw < 1 {
		let _ = server.send_privmsg(&chan, format!("chromas, is that you? stop being a wiseass.").as_str());
				return;
	}

	for pass in 1..throw {
		let mut total: u64 = 0_u64;
		for _ in 1..dice {
			let bignum = rng.gen::<u64>();
			let thisdie = (bignum % side) + 1;
			total += thisdie;
		}
		let _ = server.send_privmsg(&chan, format!("pass {}: {}", pass, total).as_str());
	}
	return;
}

fn command_youtube(server: &IrcServer, chan: &String, query: String) {
	match BOTCONFIG.lock() {
		Err(err) => println!("Could not lock BOTCONFIG: {:#?}", err),
		Ok(botconfig) => {
			let link = get_youtube(&botconfig.go_key, &query);
			let _ = server.send_privmsg(&chan, format!("https://www.youtube.com/watch?v={}", link).as_str());
		},
	};
	return;
}

fn command_submit(server: &IrcServer, chan: &String, subtx: &Sender<Submission>, suburl: String, summary: String, submitter: &String) {
	let page: String = sub_get_page(&suburl);
	if &page[0..3] == "111" {
		let _ = server.send_privmsg(&chan, &format!("{}", &page[3..]));
		return;
	}
	let title: String = sub_get_title(&page);
	if title == "".to_string() {
		let _ = server.send_privmsg(&chan, "Unable to find a title for that page");
		return;
	}

	let description: String = sub_get_description(&page);
	if description == "".to_string() {
		let _ = server.send_privmsg(&chan, "Unable to find a summary for that page");
		return;
	}
	
	let mut cookie;
	let botnick;
	{
		match BOTSTATE.lock() {
			Err(err) => {
				println!("Could not lock BOTSTATE: {:#?}", err);
				return;
			},
			Ok(botstate) => cookie = botstate.cookie.clone(),
		};
		match BOTCONFIG.lock() {
			Err(err) => {
				println!("Could not lock BOTCONFIG: {:#?}", err);
				return;
			},
			Ok(botconfig) => botnick = botconfig.nick.clone(),
		};
	}
	if cookie == "".to_string() {
		cookie = sub_get_cookie();
	}
	
	let reskey = sub_get_reskey(&cookie);
	if reskey == "".to_string() {
		let _ = server.send_privmsg(&chan, "Unable to get a reskey. Check the logs.");
		return;
	}
	
	let story = sub_build_story(&submitter, &description, &summary, &suburl );
	
	let submission = Submission {
		reskey: reskey,
		subject: title,
		story: story,
		chan: chan.clone(),
		cookie: cookie,
		botnick: botnick,
	};

	let _ = server.send_privmsg(&chan, "Submitting. There is a mandatory delay, please be patient.");
	
	let foo = subtx.send(submission);
	match foo {
		Ok(_) => {},
		Err(err) => println!("{:#?}", err),
	};
	return;
}

fn command_quit(server: &IrcServer, chan: String) {
	let _ = server.send_privmsg(&chan, "Your wish is my command...");
	let _ = server.send_quit("");
}

fn command_pissoff(server: &IrcServer, chan: String) {
	let _ = server.send_privmsg(&chan, "Off I shall piss...");
	let _ = server.send_quit("");
}

fn command_dieinafire(server: &IrcServer, chan: String) {
	let _ = server.send_action(&chan, "dies a firey death");
	let _ = server.send_quit("");
}

fn command_join(server: &IrcServer, joinchan: String) {
	let _ = server.send_join(&joinchan);
}

fn command_part(server: &IrcServer, chan: &String, partchan: String) {
	match BOTCONFIG.lock() {
		Err(err) => {
			println!("Could not lock BOTCONFIG: {:#?}", err);
			return;
		},
		Ok(botconfig) => {
			if botconfig.protected.contains(&partchan) {
				let _ = server.send_privmsg(&chan, "No.");
				return;
			}
		},
	};
	
	// else
	let partmsg: Message = Message {
		tags: None,
		prefix: None,
		command: Command::PART(partchan, None), 
	};
	let _ = server.send(partmsg);
	return;
}

fn command_say(server: &IrcServer, chan: String, message: String) {
	let _ = server.send_privmsg(&chan, message.as_str());
	return;
}

fn command_seen(server: &IrcServer, chan: &String, who: String) {
	struct SeenResult {
		channel: String,
		said: String,
		datetime: String,
		nick: String,
		action: bool
	};
	let result: SeenResult;
	if !sql_table_check("seen".to_string()) {
		println!("`seen` table not found, creating...");
		if !sql_table_create("seen".to_string()) {
			let _ = server.send_privmsg(&chan, "No seen table exists and for some reason I cannot create one");
			return;
		}
	}

	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			let count: i32 = conn.query_row("SELECT count(nick) FROM seen WHERE nick = ?", &[&who], |row| {
				row.get(0)
			}).unwrap();
			if count == 0 {
				let privmsg = format!("Sorry, I have not seen {}", who);
				let _ = server.send_privmsg(&chan, &privmsg);
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
				let _ = server.send_privmsg(&chan, &privmsg);
			}
			else {
				let privmsg = format!("[{}] {} <{}> {}", result.datetime, result.channel, result.nick, result.said);
				let _ = server.send_privmsg(&chan, &privmsg);
			}
			return;
		},
	};
}

fn command_smake(server: &IrcServer, chan: &String, who: String) {
	if !sql_table_check("smakes".to_string()) {
		println!("`smakes` table not found, creating...");
		if !sql_table_create("smakes".to_string()) {
			let _ = server.send_privmsg(&chan, "No smakes table exists and for some reason I cannot create one");
			return;
		}
	}

	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			let check: i32 = conn.query_row("select count(*) from smakes", &[], |row| {
				row.get(0)
			}).unwrap();
			if check == 0 {
				let _ = server.send_privmsg(&chan, "No smakes in the database, add some.");
			}

			let result: String = conn.query_row("select smake from smakes order by random() limit 1", &[], |row| {
				row.get(0)
			}).unwrap();

			let dome: String = format!("smakes {} upside the head with {}", who, result);
			let _ = server.send_action(&chan, &dome);
		},
	};
}

fn command_smakeadd(server: &IrcServer, chan: &String, what: String) {
	if !sql_table_check("smakes".to_string()) {
		println!("`smakes` table not found, creating...");
		if !sql_table_create("smakes".to_string()) {
			let _ = server.send_privmsg(&chan, "No smakes table exists and for some reason I cannot create one");
			return;
		}
	}
	
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			match conn.execute("INSERT INTO smakes VALUES(NULL, $1)", &[&what]) {
				Err(err) => {
					println!("{}", err);
					let _ = server.send_privmsg(&chan, "Error writing to smakes table.");
					return;
				},
				Ok(_) => {
					let sayme: String = format!("\"{}\" added.", what);
					let _ = server.send_privmsg(&chan, &sayme);
					return;
				},
			};
		},
	};
}

fn command_fake_weather_add(server: &IrcServer, chan: &String, what: String) {
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
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
					let _ = server.send_privmsg(&chan, "Error writing to fake_weather table.");
					return;
				},
				Ok(_) => {
					let entry: CacheEntry = CacheEntry {
						age: std::i64::MAX,
						location: location.clone(),
						weather: weather.clone(),
					};
					match WUCACHE.lock() {
						Err(err) => {
							println!("Could not lock WUCACHE: {:#?}", err);
							return;
						},
						Ok(mut wucache) => {
							wucache.push(entry);
						},
					};
					let sayme: String = format!("\"{}\" added.", location);
					let _ = server.send_privmsg(&chan, &sayme);
					return;
				},
			};
		},
	};
}

fn command_weather_alias(server: &IrcServer, chan: &String, walias: String) {
	if !sql_table_check("weather_aliases".to_string()) {
		println!("weather_aliases table not found, creating...");
		if !sql_table_create("weather_aliases".to_string()) {
			let _ = server.send_privmsg(&chan, "No weather_aliases table exists and for some reason I cannot create one");
			return;
		}
	}
	
	let mut colon = walias.find(':').unwrap_or(walias.len());
	if colon == walias.len() {
		command_help(&server, &chan, Some("weatheralias".to_string()));
		return;
	}
	let flocation = walias[..colon].trim().to_string();
	colon += 1;
	let rlocation = walias[colon..].trim().to_string();
	if flocation.len() < 3 || rlocation.len() < 3 {
		command_help(&server, &chan, Some("weatheralias".to_string()));
		return;
	}
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			// make sure an alias doesn't stomp on a saved person/place name
			let is_user: i32 = conn.query_row("SELECT count(nick) FROM locations WHERE nick = $1", &[&flocation], |row| {
				row.get(0)
			}).unwrap();
			if is_user != 0 {
				let sayme = format!("{} is someone's nick, jackass.", &flocation);
				let _ = server.send_privmsg(&chan, &sayme);
				return;
			}
			match conn.execute("REPLACE INTO weather_aliases VALUES ($1, $2 )", &[&flocation, &rlocation]) {
				Err(err) => {
					println!("{}", err);
					let _ = server.send_privmsg(&chan, "Error writing to weather_aliases table.");
					return;
				},
				Ok(_) => {
					let sayme: String = format!("\"{}\" added.", flocation);
					let _ = server.send_privmsg(&chan, &sayme);
					return;
				},
			};
		},
	};
}

fn command_weatheradd(server: &IrcServer, nick: &String, chan: &String, checklocation: String) {
	if !sql_table_check("locations".to_string()) {
		println!("locations table not found, creating...");
		if !sql_table_create("locations".to_string()) {
			let _ = server.send_privmsg(&chan, "No locations table exists and for some reason I cannot create one");
			return;
		}
	}

	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			match conn.execute("REPLACE INTO locations VALUES($1, $2)", &[nick, &checklocation]) {
				Err(err) => {
					println!("{}", err);
					let _ = server.send_privmsg(&chan, "Error saving your location.");
				},
				Ok(_) => {
					let sayme: String = format!("Location for {} set to {}", nick, checklocation);
					let _ = server.send_privmsg(&chan, &sayme);
				},
			};
			return;
		},
	};
}

fn command_weather(server: &IrcServer, nick: &String, chan: &String, checklocation: Option<String>) {
	let weather: String;
	let mut unaliasedlocation = checklocation;
	let location: Option<String>;

	// unalias unaliasedlocation if it is aliased
	match CONN.lock() {
	Err(err) => {
		println!("Could not lock CONN: {:#?}", err);
		return;
	},
	Ok(conn) => {
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
				Some(var) => {
					weather = get_weather(var.trim().to_string());
				},
				None => weather = format!("No location found for {}", nick).to_string(),
			};

			let _ = server.send_privmsg(&chan, &weather.trim().to_string());
			return;
		},
	};
}

fn command_abuser(server: &IrcServer, chan: &String, abuser: String) {
	if hostmask_add(&server, &chan, "abusers", &abuser) {
		let result: String = format!("Added '{}' to abusers.", &abuser);
		let _ = server.send_privmsg(&chan, &result);
	}
	else {
		let result: String = format!("Failed to add '{}' to abusers. Check the logs.", &abuser);
		let _ = server.send_privmsg(&chan, &result);
	}
	return;
}

/*
fn command_admin(server: &IrcServer, chan: &String, admin: String) {
	if hostmask_add(&server, &chan, "admins", &admin) {
		let result: String = format!("Added '{}' to admins.", &admin);
		let _ = server.send_privmsg(&chan, &result);
	}
	else {
		let result: String = format!("Failed to add '{}' to admins. Check the logs.", &admin);
		let _ = server.send_privmsg(&chan, &result);
	}
	return;
}
*/

fn command_bot(server: &IrcServer, chan: &String, bot: String) {
	if hostmask_add(&server, &chan, "bots", &bot) {
		let result: String = format!("Added '{}' to bots.", &bot);
		let _ = server.send_privmsg(&chan, &result);
	}
	else {
		let result: String = format!("Failed to add '{}' to bots. Check the logs.", &bot);
		let _ = server.send_privmsg(&chan, &result);
	}
	return;
}

fn command_help(server: &IrcServer, chan: &String, command: Option<String>) {
	match BOTCONFIG.lock() {
		Err(err) => {
			println!("Could not lock BOTCONFIG: {:#?}", err);
		},
		Ok(botconfig) => {
			let helptext: String = get_help(&botconfig.prefix, command);
			let _ = server.send_privmsg(&chan, &helptext);
		},
	};
	return;
}

fn sql_table_check(table: String) -> bool {
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return false;
		},
		Ok(conn) => {
			let result: i32 = conn.query_row("SELECT count(name) FROM sqlite_master WHERE type = 'table' and name = ? LIMIT 1", &[&table], |row| {
				row.get(0)
			}).unwrap();
	
			if result == 1 {
				return true;
			}
			return false;
		},
	};
}

fn sql_table_create(table: String) -> bool {
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return false;
		},
		Ok(conn) => {
			let schema: String = sql_get_schema(&table);
			match conn.execute(&schema, &[]) {
				Err(err) => { println!("{}", err); return false; },
				Ok(_) => { println!("{} table created", table); return true; },
			};
		},
	};
}

fn prime_weather_cache() {
	let table = "fake_weather".to_string();
	if !sql_table_check(table.clone()) {
		if !sql_table_create(table.clone()) {
			println!("Could not create table 'fake_weather'!");
			return;
		}
		return;
	}
	match CONN.lock() {
		Err(err) => println!("Could not lock CONN: {:#?}", err),
		Ok(conn) => {
			let mut statement = format!("SELECT count(*) FROM {}", &table);
			let result: i32 = conn.query_row(statement.as_str(), &[], |row| {
						row.get(0)
			}).unwrap();
			if result == 0 {
				return;
			}

			statement = format!("SELECT * from {}", &table);
			let mut stmt = conn.prepare(statement.as_str()).unwrap();
			let allrows = stmt.query_map(&[], |row| {
				CacheEntry {
					age: std::i64::MAX,
					location: row.get(0),
					weather: row.get(1),
				}
			}).unwrap();

			for entry in allrows {
				let thisentry = entry.unwrap();
				match WUCACHE.lock() {
					Err(err) => {
						println!("Could not lock WUCACHE: {:#?}", err);
						return;
					},
					Ok(mut wucache) => wucache.push(thisentry),
				};
			}
		},
	};
}

fn check_messages(nick: &String) -> bool {
	let table = "messages".to_string();
	if !sql_table_check(table.clone()) {
		return false;
	}
	match CONN.lock() {
		Err(err) => println!("Could not lock CONN: {:#?}", err),
		Ok(conn) => {
			let statement: String = format!("SELECT count(*) FROM {} WHERE recipient = $1", &table);
			let result: i32 = conn.query_row(statement.as_str(), &[&nick.as_str()], |row| {
				row.get(0)
			}).unwrap();
			if result > 0 {
				return true;
			}
		},
	};
	return false;
}

fn deliver_messages(server: &IrcServer, nick: &String) {
	match CONN.lock() {
		Err(err) => println!("Could not lock CONN: {:#?}", err),
		Ok(conn) => {
			struct Row {
				sender: String,
				message: String,
				ts: i64
			};
			let mut timestamps: Vec<i64> = vec![];
			let mut stmt = conn.prepare(format!("SELECT * FROM messages WHERE recipient = '{}' ORDER BY ts", &nick).as_str()).unwrap();
			let allrows = stmt.query_map(&[], |row| {
				Row {
					sender: row.get(0),
					message: row.get(2),
					ts: row.get(3)
				}
			}).unwrap();

			for row in allrows {
				let thisrow = row.unwrap();
				let _ = server.send_privmsg(&nick, format!("<{}> {}", thisrow.sender, thisrow.message).as_str());
				timestamps.push(thisrow.ts);
			}
	
			for ts in timestamps.iter() {
				let statement = format!("DELETE FROM messages WHERE recipient = '{}' AND ts = {}", &nick, &ts);
				match conn.execute(statement.as_str(), &[]) {
					Err(err) => println!("{}", err),
					Ok(_) => {}
				};
			}
		},
	};
	return;
}

fn save_msg(fromwho: &String, tellwho: String, tellwhat: String) -> bool {
	let table = "messages".to_string();
	if !sql_table_check(table.clone()) {
		if !sql_table_create(table.clone()) {
			return false;
		}
	}
	match CONN.lock() {
		Err(err) => println!("Could not lock CONN: {:#?}", err),
		Ok(conn) => {
			let time: i64 = time::now_utc().to_timespec().sec;
			let statement: String = format!("INSERT INTO {} VALUES($1, $2, $3, $4)", &table).to_string();
			match conn.execute(statement.as_str(), &[&fromwho.as_str(), &tellwho, &tellwhat, &time]) {
				Err(err) => {
					println!("{}", err);
					return false;
				},
				Ok(_) => return true,
			};
		},
	};
	return false;
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
		"abuser" => format!("Limits the commands a jackass can use. {}abuser <full @hostmask> e.g. {}abuser @Soylent/Staff/Editor/cmn32480", prefix, prefix),
		"bot" => format!("Registers a hostmask as a bot. {}bot <full @hostmask> e.g. {}bot @universe2.us/ircbot/aqu4", prefix, prefix),
		"admin" => format!("Give someone godlike powers over this bot. {}admin <full @hostmask> e.g. {}admin @Soylent/Staff/Developer/TMB", prefix, prefix),
		"youtube" => format!("Search youtube. {}youtube <search string>", prefix),
		"socialist" => format!("Has half of a libertarian debate. {}socialist <nick>", prefix),
		"roll" => format!("Roll the dice. {}roll 3d6 throws=6", prefix),
		"bnk" => format!("BOOBIES n KITTEHS!"),
		"join" => format!("{}join <channel>", prefix),
		"part" => format!("{}part or {}part <channel>", prefix, prefix),
		"tell" => format!("{}tell <nick> <message>", prefix),
		"klingon" => format!("Translate something to klingon. {}klingon <phrase>", prefix),
		"g" => format!("Google search. {}g <search query>", prefix),
		"sammich" => format!("No need for sudo..."),
		"sammichadd" => format!("Add a sammich to the db. {}sammichadd <type of sammich>", prefix),
		"say" => format!("{}say <channel/nick> <stuff>", prefix),
		"pissoff" => format!("Alias for {}quit", prefix),
		"dieinafire" => format!("Alias for {}quit", prefix),
		"quit" => format!("Pretty self-explanatory"),
		"reloadregexes" => format!("Reloads the regexes for matching title and description of a page for {}submit from disk", prefix),
		"nelson" => format!("{}nelson <with or without a nick>", prefix),
		"fitectl" => format!("{}fitectl status/scoreboard/weapon <something>/armor <something>", prefix),
		"fite" => format!("Fite someone in #fite. {}fite <nick>", prefix),
		"goodfairy" => format!("Revive everyone in the {}prefix fite game. Admin only.", prefix),
		_ => format!("{}{} is not a currently implemented command", prefix, inside),
	}
}

fn sql_get_schema(table: &String) -> String {
	match &table[..] {
		"seen" => "CREATE TABLE seen(nick TEXT, hostmask TEXT, channel TEXT, said TEXT, ts UNSIGNED INT(8), action UNSIGNED INT(1) CHECK(action IN(0,1)), primary key(nick, channel) )".to_string(),
		"smakes" => "CREATE TABLE smakes (id INTEGER PRIMARY KEY AUTOINCREMENT, smake TEXT NOT NULL)".to_string(),
		"sammiches" => "CREATE TABLE sammiches (id INTEGER PRIMARY KEY AUTOINCREMENT, sammich TEXT NOT NULL)".to_string(),
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
		"fite" => "CREATE TABLE fite(nick TEXT PRIMARY KEY NOT NULL, level UNSIGNED INT, hp UNSIGNED INT, weapon TEXT NOT NULL DEFAULT 'fist', armor TEXT NOT NULL DEFAULT ;grungy t-shirt', ts UNSIGNED INT)".to_string(),
		_ => "".to_string(),
	}
}

fn cache_push(location: &String, weather: &String) {
	cache_prune();
	match WUCACHE.lock() {
		Err(err) => {
			println!("Could not lock WUCACHE: {:#?}", err);
			return;
		},
		Ok(mut cache) => {
			let entry = CacheEntry {
				age: time::now_utc().to_timespec().sec,
				location: location.to_string().clone().to_lowercase(),
				weather: weather.to_string().clone().to_lowercase(),
			};
			cache.push(entry);
		},
	};
	return;
}

fn cache_get(location: &String) -> Option<String> {
	cache_prune();
	match WUCACHE.lock() {
		Err(err) => {
			println!("Could not lock WUCACHE: {:#?}", err);
			return None;
		},
		Ok(cache) => {
			let position: Option<usize> = cache.iter().position(|ref x| x.location == location.to_string().clone().to_lowercase());
			if position.is_some() {
				let weather: &String = &cache[position.unwrap()].weather;
				return Some(weather.to_string().clone());
			}
			return None;
		},
	};
}

fn cache_prune() {
	match WUCACHE.lock() {
		Err(err) => {
			println!("Could not lock WUCACHE: {:#?}", err);
			return;
		},
		Ok(mut cache) => {
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
		},
	};
}

fn get_weather(location: String) -> String {
	// short out if we've done this loc recently
	let cached = cache_get(&location);
	if cached.is_some() {
		return cached.unwrap();
	}

	// Get config stuffs
	let ockey: String;
	match BOTCONFIG.lock() {
		Err(err) => {
			println!("Could not lock BOTCONFIG: {:#?}", err);
			ockey = "".to_string();
			return format!("Could not lock BOTCONFIG");
		},
		Ok(botconfig) => {
			ockey = botconfig.oc_key.clone();
		}
	}
	let dwkey: String;
	match BOTCONFIG.lock() {
		Err(err) => {
			println!("Could not lock BOTCONFIG: {:#?}", err);
			dwkey = "".to_string();
			return format!("Could not lock BOTCONFIG");
		},
		Ok(botconfig) => {
			dwkey = botconfig.dw_key.clone();
		}
	}

	// gotta get the lat/long for location for the forecast crate
	let oc = Opencage::new(ockey);
	let ocres: Vec<Point<f32>> = oc.forward(&location.as_str()).unwrap();
	if ocres.len() == 0 {
		return format!("Could not get geo coordinates for '{}'.", &location.as_str());
	}
	let lat: f32 = ocres[0].y();
	let lng: f32 = ocres[0].x();
	let ocresb = oc.reverse(&Point::new(lng, lat));
	let checkedloc: String;
	if ocresb.is_err() {
		checkedloc = location.clone();
	} else {
		checkedloc = ocresb.unwrap();
	}

	// now get the weather for lat/lng
	let mut dst = Vec::new();
	let mut easy = Easy::new();
	let url = format!("https://api.darksky.net/forecast/{}/{},{}?exclude=currently%2Cminutely%2Chourly&lang=en&units=us", dwkey, lat, lng);
	easy.url(url.as_str()).unwrap();
	easy.forbid_reuse(true).unwrap();
	easy.progress(true).unwrap();
	{
		let transfer = easy.transfer();
		body_only(transfer, &mut dst);
	}
	if easy.response_code().unwrap_or(999) != 200 {
		println!("got http response code {} in command_google", easy.response_code().unwrap_or(999));
		return format!("Unable to get weather for {}", &location);
	}

	let json = str::from_utf8(&dst[..]).unwrap_or("");
	let jsonthing = Json::from_str(json).unwrap_or(Json::from_str("{}").unwrap());
	let found = jsonthing.find_path(&[&"daily", &"data"]);
	if found.is_none() {
		println!("buggy json: {:#?}", &jsonthing);
		return format!("Could not get weather for {}", &location);
	}
	let days = found.unwrap();
	let offset: i32 = jsonthing.find("offset").unwrap().as_i64().unwrap() as i32;


	let tomorrow = &days[1];
	let dayafter = &days[2];
	let prezero = Json::from_str("{\"zero\":\"0\"}").unwrap();
	let zero = prezero.find("zero").unwrap();
	let tomorrow_d: String = format!("{}", Local.timestamp(tomorrow.find("time").unwrap_or(&zero).as_i64().unwrap(), 0).with_timezone(&FixedOffset::east_opt(offset * 3600i32).unwrap()).format("%a %b %d"));
	let dayafter_d: String = format!("{}", Local.timestamp(dayafter.find("time").unwrap_or(&zero).as_i64().unwrap(), 0).with_timezone(&FixedOffset::east_opt(offset * 3600i32).unwrap()).format("%a %b %d"));
	let forecast_text = format!("{} - {} {} {}",
		format!("{}", &checkedloc),
		format!("Today: {} {}/{}F, Humidity: {}%, Precip: {}%, Wind ~{}mph.",
			days[0].find("summary").unwrap_or(&zero).to_string().trim(),
			days[0].find("temperatureHigh").unwrap_or(&zero).as_f64().unwrap() as i64,
			days[0].find("temperatureLow").unwrap_or(&zero).as_f64().unwrap() as i64,
			(days[0].find("humidity").unwrap_or(&zero).as_f64().unwrap() * 100f64) as i64,
			(days[0].find("precipProbability").unwrap_or(&zero).as_f64().unwrap() * 100f64) as i64,
			days[0].find("windSpeed").unwrap_or(&zero).as_f64().unwrap() as i64
		),
		format!("{}: {} {}/{}F, Humidity: {}%, Precip: {}%, Wind ~{}mph.",
			&tomorrow_d[..3],
			days[1].find("summary").unwrap_or(&zero).to_string().trim(),
			days[1].find("temperatureHigh").unwrap_or(&zero).as_f64().unwrap() as i64,
			days[1].find("temperatureLow").unwrap_or(&zero).as_f64().unwrap() as i64,
			(days[1].find("humidity").unwrap_or(&zero).as_f64().unwrap() * 100f64) as i64,
			(days[1].find("precipProbability").unwrap_or(&zero).as_f64().unwrap() * 100f64) as i64,
			days[1].find("windSpeed").unwrap_or(&zero).as_f64().unwrap() as i64
		),
		format!("{}: {} {}/{}F, Humidity: {}%, Precip: {}%, Wind ~{}mph.",
			&dayafter_d[..3],
			days[2].find("summary").unwrap_or(&zero).to_string().trim(),
			days[2].find("temperatureHigh").unwrap_or(&zero).as_f64().unwrap() as i64,
			days[2].find("temperatureLow").unwrap_or(&zero).as_f64().unwrap() as i64,
			(days[2].find("humidity").unwrap_or(&zero).as_f64().unwrap() * 100f64) as i64,
			(days[2].find("precipProbability").unwrap_or(&zero).as_f64().unwrap() * 100f64) as i64,
			days[2].find("windSpeed").unwrap_or(&zero).as_f64().unwrap() as i64
		)
	);

	cache_push(&location, &forecast_text);
	return forecast_text;
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
		let citystate = format!("{}/{}", encstate.trim_start_matches(",").trim(), enccity.trim()).to_string();
		return citystate;
	}
	else {
		let failed = "dohelp".to_string();
		return failed;
	}
}

fn hostmask_add(server: &IrcServer, chan: &String, table: &str, hostmask: &String) -> bool {
	if !sql_table_check(table.to_string()) {
		println!("{} table not found, creating...", table);
		if !sql_table_create(table.to_string()) {
			let err: String = format!("No {} table exists and for some reason I cannot create one. Check the logs.", table);
			let _ = server.send_privmsg(&chan, &err);
			return false;
		}
	}
	
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return false;
		},
		Ok(conn) => {
			let statement: String = format!("INSERT INTO {} VALUES($1)", &table).to_string();
			match conn.execute(statement.as_str(), &[&hostmask.as_str()]) {
				Err(err) => {
					println!("{}", err);
					return false;
				},
				Ok(_) => return true,
			};
		},
	};
}

fn is_admin(nick: &String) -> bool {
	match BOTCONFIG.lock() {
		Err(err) => {
			println!("Could not lock BOTCONFIG: {:#?}", err);
			false
		},
		Ok(botconfig) => {
			botconfig.owners.contains(nick)
		}
	}
}

fn is_bot(server: &IrcServer, chan: &String, hostmask: &String) -> bool {
	let table = "bots".to_string();
	if !sql_table_check(table.clone()) {
		println!("{} table not found, creating...", &table);
		if !sql_table_create(table.clone()) {
			let err: String = format!("No {} table exists and for some reason I cannot create one. Check the logs.", &table);
			let _ = server.send_privmsg(&chan, &err);
			return false;
		}
	}
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return false;
		},
		Ok(conn) => {
			let statement: String = format!("SELECT count(*) FROM {} WHERE hostmask = $1", &table);
			let hostmask: String = hostmask_only(&hostmask);
			let result: i32 = conn.query_row(statement.as_str(), &[&hostmask], |row| {
					row.get(0)
			}).unwrap();
			if result == 1 {
				return true;
			}
			return false;
		},
	};
}

fn is_abuser(server: &IrcServer, chan: &String, hostmask: &String) -> bool {
	let table = "abusers".to_string();
	if !sql_table_check(table.clone()) {
		println!("{} table not found, creating...", &table);
		if !sql_table_create(table.clone()) {
			let err: String = format!("No {} table exists and for some reason I cannot create one. Check the logs.", &table);
			let _ = server.send_privmsg(&chan, &err);
			return false;
		}
	}
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return false;
		},
		Ok(conn) => {
			let statement: String = format!("SELECT count(*) FROM {} WHERE hostmask = $1", &table);
			let hostmask: String = hostmask_only(&hostmask);
			let result: i32 = conn.query_row(statement.as_str(), &[&hostmask], |row| {
					row.get(0)
			}).unwrap();
			if result == 1 {
				return true;
			}
			return false;
		},
	};
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
	easy.forbid_reuse(true).unwrap();
	easy.follow_location(true).unwrap();
	easy.max_redirections(10_u32).unwrap();
	easy.max_filesize(6291456_u64).unwrap();
	easy.timeout(Duration::new(5,0)).unwrap();
	easy.progress(true).unwrap();
	{
		let transfer = easy.transfer();
		body_only(transfer, &mut dst);
	}
	if dst.len() < 10 {
		return format!("111page too large");
	}
	if easy.response_code().unwrap_or(999) != 200 {
		return format!("111got http response code {}", easy.response_code().unwrap_or(999));
	}
	let result = str::from_utf8(&dst[..]);
	let page;
	let pagestring;
	match result {
		Ok(res) => {
			page = res;
			pagestring = page.to_string().trim().to_string();
		},
		Err(e) => {
			println!("{:#?}", e);
			pagestring = format!("{}", String::from_utf8_lossy(&dst[..]));
		}
	}
	return pagestring;
}

fn sub_get_title(page: &String) -> String {
	let mut title = "".to_string();
	match TITLERES.lock() {
		Err(err) => {
			println!("Could not lock TITLERES: {:#?}", err);
			return "".to_string();
		},
		Ok(titleres) => {
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
		},
	};
}

fn sub_get_description(page: &String) -> String {
	let mut desc = "".to_string();
	match DESCRES.lock() {
		Err(err) => {
			println!("Could not lock DESCRES: {:#?}", err);
			return "".to_string();
		},
		Ok(descres) => {
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
		},
	};
}

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
	easy.forbid_reuse(true).unwrap();
	easy.cookie(cookie.as_str()).unwrap();
	easy.progress(true).unwrap();
	{
		let transfer = easy.transfer();
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

fn sub_get_cookie() -> String {
	let mut cookie = "".to_string();
	match BOTSTATE.lock() {
		Err(err) => {
			println!("Could not lock BOTSTATE: {:#?}", err);
			return "".to_string();
		},
		Ok(mut botstate) => {
			if botstate.cookie != "".to_string() {
				return botstate.cookie.clone();
			}
			let url: String;
			match BOTCONFIG.lock() {
				Err(err) => {
					println!("Could not lock CONN: {:#?}", err);
					return "".to_string();
				},
				Ok(botconfig) => {
					if DEBUG {
						url = format!("https://dev.soylentnews.org/api.pl?m=auth&op=login&nick={}&pass={}", "MrPlow", &botconfig.pass).to_string();
					}
					else {
						url = format!("https://soylentnews.org/api.pl?m=auth&op=login&nick={}&pass={}", &botconfig.nick, &botconfig.pass).to_string();
					}
				},
			};
			let mut dst = Vec::new();
			let mut easy = Easy::new();
			easy.url(url.as_str()).unwrap();
			easy.forbid_reuse(true).unwrap();
			{
				let transfer = easy.transfer();
				headers_only(transfer, &mut dst);
			}
			if easy.response_code().unwrap_or(999) != 200 {
				println!("got http response code {}", easy.response_code().unwrap_or(999));
				return "".to_string();
			}

			let headers = str::from_utf8(&dst[..]).unwrap_or("").split("\n");
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
				botstate.cookie = cookie.clone();
			}
			return cookie;
		},
	};
}

fn load_titleres() {
	let titleresf = OpenOptions::new().read(true).write(true).create(true).open("/home/bob/etc/snbot/titleres.txt");
	if titleresf.is_err() {
		println!("Error opening titleres.txt: {:#?}", titleresf);
		return;
	}
	let unwrapped = &titleresf.unwrap();
	let titleresfile = BufReader::new(unwrapped);

	match TITLERES.lock() {
		Err(err) => {
			println!("Could not lock TITLERES: {:#?}", err);
			return;
		},
		Ok(mut titleres) => {
			titleres.clear();
			for line in titleresfile.lines() {
				if line.is_ok() {
					titleres.push(Regex::new(line.unwrap().as_str()).unwrap());
				}
			}
		},
	};
}

fn load_descres() {
	let descresf = OpenOptions::new().read(true).write(true).create(true).open("/home/bob/etc/snbot/descres.txt");
	if descresf.is_err() {
		println!("Error opening descres.txt: {:#?}", descresf);
		return;
	}
	let damnit = &descresf.unwrap();
	let descresfile = BufReader::new(damnit);

	match DESCRES.lock() {
		Err(err) => {
			println!("Could not lock DESCRES: {:#?}", err);
			return;
		},
		Ok(mut descres) => {
			descres.clear();
			for line in descresfile.lines() {
				if line.is_ok() {
					descres.push(Regex::new(line.unwrap().as_str()).unwrap());
				}
			}
		},
	};
}

fn load_fiteeffects() {
	let mut infileres = OpenOptions::new().read(true).write(true).create(true).open("/home/bob/etc/snbot/fiteeffects.txt");
	if infileres.is_err() {
		println!("problem opening fiteeffects.txt: {:#?}", &outfileres.err());
		return;
	}
	let mut infile = infileres.unwrap();
	let mut out: Vec<FiteEffect>;
	match FITEEFFECTS.lock() {
		Err(err) => {
			println!("Could not lock FITEEFFECTS: {:#?}", err);
			return;
		},
		Ok(mut effects) => {
			effects.clear();
			outres = serde_json::from_reader(&infile);
			if outres.is_err() {
				println!("error reading from fiteeffects.txt: {:#?}", &outres);
				return;
			}
			effects = outres.unwrap();
}

fn get_youtube(go_key: &String, query: &String) -> String {
	let mut dst = Vec::new();
	let mut easy = Easy::new();
	let querybytes = query.clone().into_bytes();
	let encquery = easy.url_encode(&querybytes[..]);
	let url = format!("https://www.googleapis.com/youtube/v3/search/?maxResults=1&q={}&order=relevance&type=video&part=snippet&key={}", encquery, go_key);
	easy.url(url.as_str()).unwrap();
	easy.forbid_reuse(true).unwrap();
	easy.progress(true).unwrap();
	easy.fail_on_error(true).unwrap();
	{
		let transfer = easy.transfer();
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
	let cleanme = format!("{} -- {}", resopt[0].find_path(&["id", "videoId"]).unwrap().as_string().unwrap().to_string(), resopt[0].find_path(&["snippet", "title"]).unwrap().as_string().unwrap().to_string());
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
	let postdata;
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
	easy.forbid_reuse(true).unwrap();
	easy.cookie(submission.cookie.as_str()).unwrap();
	easy.post_field_size(postdata.len() as u64).unwrap();
	easy.post_fields_copy(postdata).unwrap();
	easy.post(true).unwrap();
	easy.fail_on_error(true).unwrap();
	easy.progress(true).unwrap();
	{
		let transfer = easy.transfer();
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

fn get_bing_token() -> String {
	let bi_key;
	let bi_bytes;
	let secretbytes;
	match BOTCONFIG.lock() {
		Err(err) => {
			println!("Could not lock BOTCONFIG: {:#?}", err);
			return "".to_string();
		},
		Ok(botconfig) => {
			bi_key = botconfig.bi_key.clone();
			bi_bytes = bi_key.as_bytes();
			secretbytes = bi_bytes;
		},
	};
	let url = "https://datamarket.accesscontrol.windows.net/v2/OAuth2-13/";
	let mut dst = Vec::new();
	let mut easy = Easy::new();
	let postfields = format!("grant_type=client_credentials&scope=http://api.microsofttranslator.com&client_id=TMBuzzard_Translator&client_secret={}", easy.url_encode(secretbytes));
	let postbytes = postfields.as_bytes();
	easy.url(url).unwrap();
	easy.forbid_reuse(true).unwrap();
	easy.post_field_size(postbytes.len() as u64).unwrap();
	easy.post_fields_copy(postbytes).unwrap();
	easy.post(true).unwrap();
	easy.progress(true).unwrap();
	{
		let transfer = easy.transfer();
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

fn is_nick_registered(server: &IrcServer, whorx: &Receiver<NSResponse>, nick: &String) -> bool{
	match BOTSTATE.lock() {
		Err(err) => println!("Error locking BOTSTATE: {:#?}", err),
		Ok(mut botstate) => {
			while botstate.ns_waiting == true {
				thread::sleep(Duration::new(1,0));
			}
			botstate.ns_waiting = true;
		},
	};
	do_who(&server, &nick.as_str());
	let returnme;
	let tenthSecond = Duration::from_millis(100);
	let mut tenths = 0;

	loop {
		match whorx.try_recv() {
			Err(_) => {
				if tenths >= 10 {
					returnme = false;
					println!("Did not get a ns response in under a second");
					break;
				}
				thread::sleep(tenthSecond);
				tenths += 1;
			},
			Ok(nsresponse) => {
				if &nsresponse.nsname[..] == "0" {
					returnme = false; 
				}
				else {
					returnme = true;
				}
				break;
			},
		};
	}
	match BOTSTATE.lock() {
		Err(err) => println!("Error locking BOTSTATE: {:#?}", err),
		Ok(mut botstate) =>	botstate.ns_waiting = false,
	};
	return returnme;
}

// Returns the number of ms until next recurrence if this is a recurring timer
fn handle_timer(server: &IrcServer, feedbacktx: &Sender<Timer>, timer: &TimerTypes) -> u64 {
	match timer {
		&TimerTypes::Action { ref chan, ref msg } => { let _ = server.send_action(&chan, &msg); return 0_u64; },
		&TimerTypes::Message { ref chan, ref msg } => { let _ = server.send_privmsg(&chan, &msg); return 0_u64; },
		&TimerTypes::Once { ref command } => {
			match &command[..] {
				"goodfairy" => {
					let chan = "#fite".to_string();
					command_goodfairy( &server, &chan );
				},
				_ => {},
			};
			return 0_u64;
		},
		&TimerTypes::Recurring { ref every, ref command } => {
			match &command[..] {
				"goodfairy" => { 
					let chan = "#fite".to_string();
					command_goodfairy( &server, &chan );
				},
				"scoreboard" => {
					fitectl_scoreboard(&server, false);
				},
				_ => {},
			};
			return every.clone() as u64;
		},
		&TimerTypes::Sendping { ref doping } => {
			if !doping { return 0_u64; }
			let timer = Timer {
				delay: 0,
				action: TimerTypes::Feedback{
					command: "fiteoff".to_string(),
				},
			};
			// send msg to turn off botconfig.is_fighting
			let _ = feedbacktx.send(timer);
			// send server ping to get us a response that will trigger a read of feedbackrx
			let _ = server.send(Message{tags: None, prefix: None, command: Command::PING("irc.soylentnews.org".to_string(), None)});
			return 0_u64;
		},
		&TimerTypes::Savechars { ref attacker, ref defender } => {
			save_character(&attacker);
			save_character(&defender);
			fitectl_scoreboard(&server, true);
			return 0_u64;
		},
		_ => {return 0_u64;},
	};
}

fn get_recurring_timers() -> Vec<TimerTypes> {
	let mut recurringTimers: Vec<TimerTypes> = Vec::new();
	match CONN.lock() {
		Err(err) => println!("Could not lock CONN: {:#?}", err),
		Ok(conn) => {
			let mut stmt = conn.prepare("SELECT * FROM recurring_timers").unwrap();
			let allrows = stmt.query_map(&[], |row| {
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
		},
	};
	let recurringTimers = recurringTimers;
	return recurringTimers;
}

// Begin fite code
fn fite(server: &IrcServer, timertx: &Sender<Timer>, attacker: &String, target: &String) -> bool {
	let spamChan = "#fite".to_string();
	let mut msgDelay = 0_u64;
	let mut oAttacker: Character = get_character(&attacker);
	let mut oDefender: Character = get_character(&target);
	let mut rAttacker: &mut Character;
	let mut rDefender: &mut Character;
	let mut surprise: bool = false;
	let mut aFumble: bool = false;
	let mut dFumble: bool = false;

	// Make sure both characters are currently alive
	if !is_alive(&oAttacker) {
		let err = format!("#fite How can you fight when you're dead? Try again tomorrow.");
		let _ = server.send_privmsg(&spamChan, &err);
		return false;
	}
	if !is_alive(&oDefender) {
		let err = format!("#fite {}'s corpse is currently rotting on the ground. Try fighting someone who's alive.", &target);
		let _ = server.send_privmsg(&spamChan, &err);
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
			let _ = timertx.send(sendme);
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

	let speeditup: u64;
	if rDefender.level > 49 && rAttacker.level > 49 {
		if rDefender.level < rAttacker.level {
			speeditup = rDefender.level / 25;
		}
		else {
			speeditup = rAttacker.level / 25;
		}	
	}
	else {
		speeditup = 1_u64;
	}

	// Do combat rounds until someone dies
	loop {
		// whoever won init's turn
		let mut attackRoll: u8 = roll_once(20_u8);
		let mut damageRoll: u64 = 0;
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
			let _ = timertx.send(sendme);
		}
		// Crit
		else if attackRoll == 20_u8 {
			for _ in 0..speeditup {
				damageRoll += roll_dmg();
			}
			damageRoll = damageRoll * 2;
			let msg = format!("{}{} smites the everlovin crap out of {} with a {} ({}04{}{})", &clearall, &anick, &dnick, &aweapon, &color, damageRoll, &color);
			if damageRoll as u64 > rDefender.hp {
				damageRoll = rDefender.hp;
			}
			rDefender.hp = rDefender.hp - damageRoll;
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			let _ = timertx.send(sendme);
		}
		// Hit
		else if attackRoll > ARMOR_CLASS {
			for _ in 0..speeditup {
				damageRoll += roll_dmg();
			}
			let msg = format!("{}{} clobbers {} upside their head with a {} ({}14{}{})", &clearall, &anick, &dnick, &aweapon, &color, damageRoll, &color);
			if damageRoll as u64 > rDefender.hp {
				damageRoll = rDefender.hp;
			}
			rDefender.hp = rDefender.hp - damageRoll;
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			let _ = timertx.send(sendme);
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
			let _ = timertx.send(sendme);
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
			let _ = timertx.send(sendme);
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
						chan: spamChan.clone(),
						msg: deathmsg,
				},
			};
			let _ = timertx.send(sendme);
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
			let _ = timertx.send(sendme);
		}
		// Crit
		else if attackRoll == 20_u8 {
			for _ in 0..speeditup {
				damageRoll += roll_dmg();
			}
			damageRoll = damageRoll * 2;
			let msg = format!("{}{} smites the everlovin crap out of {} with a {} ({}04{}{})", &clearall, &dnick, &anick, &dweapon, &color, damageRoll, &color);
			if damageRoll as u64 > rAttacker.hp {
				damageRoll = rAttacker.hp;
			}
			rAttacker.hp = rAttacker.hp - damageRoll;
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			let _ = timertx.send(sendme);
		}
		// Hit
		else if attackRoll > ARMOR_CLASS {
			for _ in 0..speeditup {
				damageRoll += roll_dmg();
			}
			let msg = format!("{}{} clobbers {} upside their head with a {} ({}14{}{})", &clearall, &dnick, &anick, &dweapon, &color, damageRoll, &color);
			if damageRoll as u64 > rAttacker.hp {
				damageRoll = rAttacker.hp;
			}
			rAttacker.hp = rAttacker.hp - damageRoll;
			let sendme: Timer = Timer {
				delay: msgDelay,
				action: TimerTypes::Message{
						chan: spamChan.clone(),
						msg: msg,
				},
			};
			let _ = timertx.send(sendme);
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
			let _ = timertx.send(sendme);
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
			let _ = timertx.send(sendme);
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
						chan: spamChan.clone(),
						msg: deathmsg,
				},
			};
			let _ = timertx.send(sendme);
			break;
		}
		msgDelay += 1000_u64;
	}
	
	// Save characters AFTER display of the last message
	let cAttacker = rAttacker.clone();
	let cDefender = rDefender.clone();
	let saveTimer = Timer {
		delay: msgDelay + 100_u64,
		action: TimerTypes::Savechars {
			attacker: cAttacker,
			defender: cDefender,
		},
	};
	let _ = timertx.send(saveTimer);
	// Send a timer to the timer handling thread with msgDelay + 100 delay so it fires just after the last
	let timer = Timer {
			delay: msgDelay + 1100_u64,
			action: TimerTypes::Sendping {
					doping: true,
			},
	};
	let _ = timertx.send(timer);
	return true;
}

fn save_character(character: &Character) {
	let time: i64 = time::now_utc().to_timespec().sec;
	let level = character.level as i64;
	let hp = character.hp as i64;
	if &character.nick.len() > &3 && &character.nick.as_str()[..4] == "NPC_" {
		return;
	}
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			conn.execute("UPDATE characters SET level = ?, hp = ?, ts = ? WHERE nick = ?", &[&level, &hp, &time, &character.nick.as_str()]).unwrap();
		},
	};
}

fn roll_once(sides: u8) -> u8 {
	let mut rng = rand::thread_rng();
	let random = rng.gen::<u64>();
	let mut roll = (random % (sides as u64)) as u8;
	roll += 1;
	return roll;
}

fn roll_dmg() -> u64 {
	let mut roll = roll_once(6_u8) as u64;
	let mut total = 0_u64;
	total += roll;
	while roll == 6 {
		roll = roll_once(6_u8) as u64;
		total += roll;
	}
	let total = total;
	return total;
}

fn character_exists(nick: &String) -> bool {
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return false;
		},
		Ok(conn) => {
			let count: i32 = conn.query_row("SELECT count(nick) FROM characters WHERE nick = ?", &[&nick.as_str()], |row| {
				row.get(0)
			}).unwrap();
			if count == 1 {
				return true;
			}
			else {
				return false;
			}
		},
	};
}

fn create_character(nick: &String) {
	let time: i64 = time::now_utc().to_timespec().sec;
	if &nick.len() > &3 && &nick[..4] == "NPC_" {
		return;
	}
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			conn.execute("INSERT INTO characters VALUES(?, 1, 10, 'fist', 'grungy t-shirt', ?)", &[&nick.as_str(), &time]).unwrap();
		},
	};
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

fn get_character(nick: &String) -> Character {
	// return a fake character if &nick[..4] == "NPC_"
	if &nick.len() > &3 && &nick[..4] == "NPC_" {
		return Character {
			nick: nick.clone(),
			level: 1,
			hp: 11,
			weapon: "dangly bits".to_string(),
			armor: "morning wood".to_string(),
			ts: 0,
			initiative: 0,
		};
	}
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return Character {
				nick: "".to_string(),
				level: 0,
				hp: 0,
				weapon: "".to_string(),
				armor: "".to_string(),
				ts: 0,
				initiative: 0,
			};
		},
		Ok(conn) => {
			let (leveli, hpi, weapon, armor, tsi) = conn.query_row("SELECT * FROM characters WHERE nick = ?", &[&nick.as_str()], |row| {
				(
					row.get(1),
					row.get(2),
					row.get(3),
					row.get(4),
					row.get(5),
				)
			}).unwrap_or((0_i64, 0_i64, "".to_string(), "".to_string(), 0_i64));
			let character: Character = Character {
				nick: nick.clone(),
				level: leveli as u64,
				hp: hpi as u64,
				weapon: weapon,
				armor: armor,
				ts: tsi as u64,
				initiative: 0_u8,
			};
			return character;
		},
	};
}

fn fitectl_scoreboard(server: &IrcServer, quiet: bool) {
	let spamChan = "#fite".to_string();
	struct Row {
		nick: String,
		lvl: i32,
		hp: i32,
		w: String,
		a: String,
	}

	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			let mut stmt = conn.prepare("SELECT * FROM characters ORDER BY level DESC, hp DESC, nick").unwrap();
			let allrows = stmt.query_map(&[], |row| {
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

			let mut outString: String = "<html><head><link rel='stylesheet' type='text/css' href='/css/fite.css'><title>#fite Scoreboard</title></head><body><table>
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
						let _ = server.send_privmsg(&spamChan, &msg);
					}
				},
				Err(err) => { println!("{}", err); },
			};
		},
	};
	return;
}

fn fitectl_status(server: &IrcServer, chan: &String, nick: &String) {
	if !character_exists(&nick) {
		create_character(&nick);
	}
	
	struct Row {
		nick: String,
		lvl: i32,
		hp: i32,
		w: String,
		a: String,
	}
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
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
			let _ = server.send_privmsg(&chan, &msg);
		},
	};
	return;
}

fn fitectl_weapon(server: &IrcServer, chan: &String, nick: &String, weapon: String) {
	if !character_exists(&nick) {
		create_character(&nick);
	}
	let saveWeapon;
	if weapon.contains("<") || weapon.contains(">") || weapon.len() > 32 {
		saveWeapon = "micro-penis".to_string();
	}
	else {
		saveWeapon = weapon;
	}
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			conn.execute("UPDATE characters SET weapon = ? WHERE nick = ?", &[&saveWeapon.as_str(), &nick.as_str()]).unwrap();
			let msg = format!("#fite weapon for {} set to {}.", &nick, &saveWeapon);
			let _ = server.send_privmsg(&chan, &msg);
		},
	};
	return;
}

fn fitectl_armor(server: &IrcServer, chan: &String, nick: &String, armor: String) {
	if !character_exists(&nick) {
		create_character(&nick);
	}
	let saveArmor;
	if armor.contains("<") || armor.contains(">") || armor.len() > 32 {
		saveArmor = "frilly lace panties".to_string();
	}
	else {
		saveArmor = armor;
	}
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return;
		},
		Ok(conn) => {
			conn.execute("UPDATE characters SET armor = ? WHERE nick = ?", &[&saveArmor.as_str(), &nick.as_str()]).unwrap();
			let msg = format!("#fite armor for {} set to {}.", &nick, &saveArmor);
			let _ = server.send_privmsg(&chan, &msg);
		},
	};
	return;
}

fn is_nick_fiter(nick: &String) -> bool {
	match CONN.lock() {
		Err(err) => {
			println!("Could not lock CONN: {:#?}", err);
			return false;
		},
		Ok(conn) =>{
			struct Row {
				count: i64,
			};
			let result: Row = conn.query_row("SELECT count(nick) FROM characters WHERE nick = ?", &[&nick.as_str()], |row| {
				Row {
					count: row.get(0)
				}
			}).unwrap();
			if result.count == 0 {
				return false;
			}
			return true;
		}
	}
}

fn register_fiter(nick: &String) -> Result<(), String> {
	match CONN.lock() {
		Err(err) => {
			let rerr = format!("{:#?}", &err);
			return Err(rerr);
		},
		Ok(conn) => {
			// FIX
			return Ok(());
		},
	};
}
