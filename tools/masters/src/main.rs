// Builds master-game statistics for the Opening Trainer.
//
// Reads PGN on stdin (e.g. `7z x -so LumbrasGigabase_OTB.7z | mstats targets.txt masters.json`),
// keeps games where both players are rated at least MIN_ELO, and for every position on the
// trainer's library lines (targets.txt) plus up to EXT plies past where a game leaves them,
// counts each move played with its results. Positions are keyed like the app does:
// "board turn castling" (the first three FEN fields, no en-passant square).

use std::{
    collections::{HashMap, HashSet},
    env, fs,
    hash::{DefaultHasher, Hash, Hasher},
    io::{self, Write},
    ops::ControlFlow,
    time::Instant,
};

use pgn_reader::{RawTag, Reader, SanPlus, Visitor};
use shakmaty::{
    fen::{Epd, Fen},
    san::SanPlus as SanP,
    uci::UciMove,
    CastlingMode, Chess, EnPassantMode, Position, Role, Square,
};

const MAX_PLY: usize = 60;

fn key_of(pos: &Chess) -> String {
    let e = Epd::from_position(pos, EnPassantMode::Legal).to_string();
    e.splitn(4, ' ').take(3).collect::<Vec<_>>().join(" ")
}
fn h(s: &str) -> u64 {
    let mut x = DefaultHasher::new();
    s.hash(&mut x);
    x.finish()
}

#[derive(Default)]
struct MoveStat { n: u32, w: u32, d: u32, b: u32, recent: u32 }

#[derive(Default)]
struct Node {
    key: Option<Box<str>>,
    n: u32,
    moves: Vec<(u16, MoveStat)>,
}

#[derive(Clone)]
struct GameInfo { white: String, black: String, welo: u32, belo: u32, year: u32, result: u8, event: String, ply: usize }

#[derive(Default)]
struct Tags { welo: u32, belo: u32, result: u8, year: u32, white: String, black: String, event: String, bad: bool }

struct Game { pos: Chess, ply: usize, last_target: usize, deepest: Option<u64>, tags: Tags }

struct Builder {
    targets: HashSet<u64>,
    nodes: HashMap<u64, Node>,
    top: HashMap<u64, Vec<GameInfo>>,
    min_elo: u32,
    ext: usize,
    ext_min: u32,
    recent_year: u32,
    seen: u64,
    used: u64,
}

fn enc(m: &UciMove) -> u16 {
    match m {
        UciMove::Normal { from, to, promotion } => {
            let p = match promotion { None => 0, Some(Role::Knight) => 1, Some(Role::Bishop) => 2, Some(Role::Rook) => 3, Some(Role::Queen) => 4, _ => 0 };
            (u16::from(*from) ) | (u16::from(*to) << 6) | (p << 12)
        }
        _ => 0,
    }
}
fn dec(c: u16) -> UciMove {
    let from = Square::new((c & 63) as u32);
    let to = Square::new(((c >> 6) & 63) as u32);
    let promotion = match c >> 12 { 1 => Some(Role::Knight), 2 => Some(Role::Bishop), 3 => Some(Role::Rook), 4 => Some(Role::Queen), _ => None };
    UciMove::Normal { from, to, promotion }
}

impl Visitor for Builder {
    type Tags = Tags;
    type Movetext = Game;
    type Output = ();

    fn begin_tags(&mut self) -> ControlFlow<(), Tags> {
        self.seen += 1;
        ControlFlow::Continue(Tags::default())
    }
    fn tag(&mut self, t: &mut Tags, name: &[u8], value: RawTag<'_>) -> ControlFlow<()> {
        let v = value.decode_utf8_lossy();
        match name {
            b"WhiteElo" => t.welo = v.trim().parse().unwrap_or(0),
            b"BlackElo" => t.belo = v.trim().parse().unwrap_or(0),
            b"Result" => t.result = match v.trim() { "1-0" => 1, "1/2-1/2" => 2, "0-1" => 3, _ => 0 },
            b"Date" => t.year = v.get(0..4).and_then(|y| y.parse().ok()).unwrap_or(0),
            b"White" => t.white = v.into_owned(),
            b"Black" => t.black = v.into_owned(),
            b"Event" => t.event = v.into_owned(),
            b"FEN" | b"SetUp" => t.bad = true,
            b"Variant" => if !v.eq_ignore_ascii_case("standard") { t.bad = true },
            _ => {}
        }
        ControlFlow::Continue(())
    }
    fn begin_movetext(&mut self, t: Tags) -> ControlFlow<(), Game> {
        if t.bad || t.result == 0 || t.welo < self.min_elo || t.belo < self.min_elo {
            return ControlFlow::Break(());
        }
        self.used += 1;
        if self.used % 500_000 == 0 { self.prune(); }
        ControlFlow::Continue(Game { pos: Chess::default(), ply: 0, last_target: 0, deepest: None, tags: t })
    }
    fn san(&mut self, g: &mut Game, sp: SanPlus) -> ControlFlow<()> {
        if g.ply >= MAX_PLY || g.ply > g.last_target + self.ext { return ControlFlow::Break(self.finish(g)); }
        let m = match sp.san.to_move(&g.pos) { Ok(m) => m, Err(_) => return ControlFlow::Break(self.finish(g)) };
        let key = key_of(&g.pos);
        let hk = h(&key);
        let is_target = self.targets.contains(&hk);
        if is_target { g.last_target = g.ply; g.deepest = Some(hk); }
        let code = enc(&m.to_uci(CastlingMode::Standard));
        let (res, year, recent_year, ext_min) = (g.tags.result, g.tags.year, self.recent_year, self.ext_min);
        let node = self.nodes.entry(hk).or_default();
        node.n += 1;
        if node.key.is_none() && (is_target || node.n >= ext_min) { node.key = Some(key.into_boxed_str()); }
        let ms = match node.moves.iter_mut().position(|(c, _)| *c == code) {
            Some(i) => &mut node.moves[i].1,
            None => { node.moves.push((code, MoveStat::default())); &mut node.moves.last_mut().unwrap().1 }
        };
        ms.n += 1;
        match res { 1 => ms.w += 1, 2 => ms.d += 1, _ => ms.b += 1 }
        if year >= recent_year { ms.recent += 1; }
        g.pos.play_unchecked(m);
        g.ply += 1;
        ControlFlow::Continue(())
    }
    fn end_game(&mut self, mut g: Game) {
        // the final position can itself be a library position
        let hk = h(&key_of(&g.pos));
        if self.targets.contains(&hk) { g.deepest = Some(hk); g.last_target = g.ply; }
        self.finish(&mut g)
    }
}

impl Builder {
    // remember the strongest games for the deepest library position each game reached
    fn finish(&mut self, g: &mut Game) {
        let Some(hk) = g.deepest.take() else { return };
        let t = &g.tags;
        let gi = GameInfo { white: t.white.clone(), black: t.black.clone(), welo: t.welo, belo: t.belo, year: t.year, result: t.result, event: t.event.clone(), ply: g.last_target };
        let list = self.top.entry(hk).or_default();
        let score = |x: &GameInfo| (x.welo + x.belo, x.year);
        if list.len() < 3 || score(&gi) > score(list.last().unwrap()) {
            list.push(gi);
            list.sort_by(|a, b| score(b).cmp(&score(a)));
            list.truncate(3);
        }
    }
    // drop one-off side positions now and then to keep memory flat
    fn prune(&mut self) {
        let before = self.nodes.len();
        let targets = &self.targets;
        self.nodes.retain(|k, n| targets.contains(k) || n.n > 1);
        eprintln!("[{:.0}s] games read {} used {} · positions {} -> {}", START.with(|s| s.elapsed().as_secs_f64()), self.seen, self.used, before, self.nodes.len());
    }
}

thread_local!(static START: Instant = Instant::now());

fn main() -> io::Result<()> {
    START.with(|_| ());
    let a: Vec<String> = env::args().collect();
    if a.len() < 3 { eprintln!("usage: mstats targets.txt out.json [minElo=2200] [ext=6] [extMin=30] [source label]"); std::process::exit(2); }
    let min_elo = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(2200);
    let ext = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(6);
    let ext_min = a.get(5).and_then(|x| x.parse().ok()).unwrap_or(30);
    let label = a.get(6).cloned().unwrap_or_default();
    let tlist: Vec<String> = fs::read_to_string(&a[1])?.lines().filter(|l| !l.is_empty()).map(String::from).collect();
    let targets: HashSet<u64> = tlist.iter().map(|k| h(k)).collect();
    let mut b = Builder { targets, nodes: HashMap::new(), top: HashMap::new(), min_elo, ext, ext_min, recent_year: 2015, seen: 0, used: 0 };
    let stdin = io::stdin();
    let mut r = Reader::new(stdin.lock());
    r.visit_all_games(&mut b)?;
    b.prune();

    // ---- write JSON ----
    let mut out = String::with_capacity(4 << 20);
    let mut kept = 0usize;
    let mut years = (u32::MAX, 0u32);
    out.push_str("{\"v\":1");
    out += &format!(",\"src\":{},\"minElo\":{},\"games\":{},\"recentYear\":{}", serde_json::to_string(&label).unwrap(), min_elo, b.used, b.recent_year);
    out.push_str(",\"pos\":{");
    let mut first = true;
    let mut keys: Vec<(&u64, &Node)> = b.nodes.iter().filter(|(k, n)| n.key.is_some() && (b.targets.contains(k) || n.n >= ext_min)).collect();
    keys.sort_by(|x, y| y.1.n.cmp(&x.1.n));
    for (hk, node) in keys {
        let key = node.key.as_deref().unwrap();
        let Ok(fen) = format!("{key} - 0 1").parse::<Fen>() else { continue };
        let Ok(pos) = fen.into_position::<Chess>(CastlingMode::Standard) else { continue };
        let is_target = b.targets.contains(hk);
        let mut mv: Vec<&(u16, MoveStat)> = node.moves.iter().collect();
        mv.sort_by(|x, y| y.1.n.cmp(&x.1.n));
        let cap = if is_target { 10 } else { 6 };
        let mut parts = Vec::new();
        for (code, s) in mv.into_iter().take(cap) {
            if s.n < 2 && parts.len() >= 3 { break; }
            let Ok(m) = dec(*code).to_move(&pos).or_else(|_| {
                // castling arrives as king-two-squares in standard UCI
                let u: UciMove = format!("{}", dec(*code)).parse().unwrap(); u.to_move(&pos)
            }) else { continue };
            let san = SanP::from_move(pos.clone(), m).to_string();
            parts.push(format!("[{},{},{},{},{},{}]", serde_json::to_string(&san).unwrap(), s.n, s.w, s.d, s.b, s.recent));
        }
        if parts.is_empty() { continue; }
        if !first { out.push(','); }
        first = false;
        kept += 1;
        out += &format!("{}:{{\"m\":[{}]", serde_json::to_string(key).unwrap(), parts.join(","));
        if let Some(gl) = b.top.get(hk) {
            let gs: Vec<String> = gl.iter().map(|g| {
                if g.year > 0 { years.0 = years.0.min(g.year); years.1 = years.1.max(g.year); }
                format!("[{},{},{},{},{},{},{}]", serde_json::to_string(&g.white).unwrap(), serde_json::to_string(&g.black).unwrap(), g.welo, g.belo, g.year, g.result, serde_json::to_string(&g.event).unwrap())
            }).collect();
            out += &format!(",\"g\":[{}]", gs.join(","));
        }
        out.push('}');
    }
    out.push_str("}}\n");
    fs::File::create(&a[2])?.write_all(out.as_bytes())?;
    eprintln!("done: games read {} used {} · positions written {} · {} bytes · notable-game years {:?}", b.seen, b.used, kept, out.len(), years);
    Ok(())
}
