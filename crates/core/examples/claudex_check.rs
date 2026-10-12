//! Read-only: reports whether Peekaboo can parse this machine's claudex output (no names printed).
use peekaboo_core::claudex::*;

fn main() {
    let c = Claudex::new(find_claudex());
    let t = std::time::Instant::now();
    let s = c.snapshot(true);
    let ok = |name: &str, parsed: bool, err: &Option<String>| println!("{name}: parsed={parsed} error={err:?}");
    println!("installed={} in {:?}", s.installed, t.elapsed());
    ok("usage", s.usage.data.is_some(), &s.usage.error);
    ok("list", s.accounts.data.is_some(), &s.accounts.error);
    ok("current", s.current.data.is_some(), &s.current.error);
    ok("pool", s.pool.data.is_some(), &s.pool.error);
    println!("autoswitch known={}", s.autoswitch.is_some());
    if let Some(u) = &s.usage.data {
        println!("accounts={} with_week_reading={}", u.len(), u.iter().filter(|r| r.week.pct.is_some()).count());
    }
}
