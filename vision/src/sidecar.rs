//! Local JSON lines. A worker writes only observations delivered to this consumer.
use crate::{
    ai::{Exchange, Report},
    pipeline::Queue,
    semantic::{GraphDelta, SemanticSceneGraph},
    source::Result,
};
use serde_json::json;
use std::{
    fs::File,
    io::{BufRead, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

type Item = (Arc<SemanticSceneGraph>, Option<Report>, Vec<Exchange>);
pub fn path(xtv: &Path) -> PathBuf {
    xtv.with_extension("semantic.jsonl")
}
pub struct Writer {
    q: Arc<Queue<Item>>,
    join: Option<thread::JoinHandle<Result<()>>>,
    pub drops: Arc<AtomicU64>,
    last: Option<(String, u64)>,
}
impl Writer {
    pub fn new(xtv: &Path) -> Result<Self> {
        Self::after(xtv, 0)
    }
    pub fn after(xtv: &Path, exchange_id: u64) -> Result<Self> {
        let f = File::create_new(path(xtv))?;
        let drops = Arc::new(AtomicU64::default());
        let q: Arc<Queue<Item>> = Queue::new(drops.clone());
        let work = q.clone();
        let lost = drops.clone();
        let join = thread::spawn(move || -> Result<()> {
            let mut out = BufWriter::new(f);
            let mut previous: Option<Arc<SemanticSceneGraph>> = None;
            let mut count = 0;
            let mut last_exchange = exchange_id;
            let mut last_report = String::new();
            while let Some((g, r, exchanges)) = work.pop() {
                let record = if let Some(base) = previous.as_ref().filter(|_| count % 60 != 0) {
                    json!({"delta":g.delta(base),"ns":g.ns,"status":g.status,"optics":g.optics,"activities":g.activities,"expired":g.expired})
                } else {
                    json!({"snapshot":g.as_ref()})
                };
                let reply = r.filter(|r| r.success && r.scene_id != last_report);
                if let Some(r) = &reply {
                    last_report = r.scene_id.clone();
                }
                let mut requests = vec![];
                for e in exchanges {
                    if e.id > last_exchange && e.response.is_some() {
                        requests.push(json!({"id":e.id,"target":&*e.target,"request":&*e.request,"status":&*e.status}));
                        last_exchange = e.id;
                    }
                }
                serde_json::to_writer(
                    &mut out,
                    &json!({"schema":"xtion.semantic-sidecar/1","ns":g.ns,"graph":record,"reply":reply,"requests":requests,"writer_drops":lost.load(Ordering::Relaxed)}),
                )?;
                out.write_all(b"\n")?;
                previous = Some(g);
                count += 1;
            }
            out.flush()?;
            Ok(())
        });
        Ok(Self {
            q,
            join: Some(join),
            drops,
            last: None,
        })
    }
    pub fn push(&mut self, g: Arc<SemanticSceneGraph>, r: Option<Report>, requests: Vec<Exchange>) {
        let key = (g.snapshot.clone(), g.ns);
        if self.last.as_ref() != Some(&key) {
            self.last = Some(key);
            self.q.push((g, r, requests));
        }
    }
    pub fn finish(&mut self) -> Result<()> {
        self.q.close();
        if let Some(t) = self.join.take() {
            t.join().map_err(|_| "semantic writer panicked")??;
        }
        Ok(())
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        if let Err(e) = self.finish() {
            eprintln!("semantic recording: {e}");
        }
    }
}
pub struct Playback<R> {
    reader: R,
    last: Option<SemanticSceneGraph>,
    next: Option<(SemanticSceneGraph, serde_json::Value)>,
}
impl<R: BufRead> Playback<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            last: None,
            next: None,
        }
    }
    fn read(&mut self) -> Result<Option<(SemanticSceneGraph, serde_json::Value)>> {
        use std::io::Read;
        let mut line = String::new();
        if self.reader.by_ref().take(8_388_609).read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line.len() > 8_388_608 {
            return Err("oversized semantic record".into());
        }
        let v: serde_json::Value = serde_json::from_str(&line)?;
        if v["schema"] != "xtion.semantic-sidecar/1" {
            return Err("unknown semantic sidecar version".into());
        }
        let record = &v["graph"];
        let g = if !record["snapshot"].is_null() {
            serde_json::from_value(record["snapshot"].clone())?
        } else {
            let mut g = self.last.take().ok_or("delta without initial snapshot")?;
            let delta: GraphDelta = serde_json::from_value(record["delta"].clone())?;
            if delta.base != g.snapshot {
                return Err("delta base mismatch".into());
            }
            g.snapshot = delta.snapshot;
            g.entities.retain(|e| !delta.removed.contains(&e.id));
            for e in delta.upsert {
                if let Some(old) = g.entities.iter_mut().find(|x| x.id == e.id) {
                    *old = e
                } else {
                    g.entities.push(e)
                }
            }
            g.entities.sort_by_key(|e| {
                delta
                    .order
                    .iter()
                    .position(|id| *id == e.id)
                    .unwrap_or(usize::MAX)
            });
            g.ns = record["ns"].as_u64().ok_or("missing timestamp")?;
            g.status = record["status"].as_str().ok_or("missing status")?.into();
            g.optics = serde_json::from_value(record["optics"].clone())?;
            g.relationships = delta.relationships;
            g.activities = serde_json::from_value(record["activities"].clone())?;
            g.expired = serde_json::from_value(record["expired"].clone())?;
            g
        };

        if v["ns"].as_u64() != Some(g.ns) || g.schema != "xtion.semantic/1" {
            return Err("invalid graph timestamp or schema".into());
        }
        self.last = Some(g.clone());
        Ok(Some((g, v)))
    }
    pub fn at(&mut self, ns: u64) -> Result<Option<(SemanticSceneGraph, Option<Report>)>> {
        loop {
            if self.next.is_none() {
                self.next = self.read()?
            }
            let Some((g, _)) = &self.next else {
                return Ok(None);
            };
            match g.ns.cmp(&ns) {
                std::cmp::Ordering::Less => self.next = None,
                std::cmp::Ordering::Greater => return Ok(None),
                std::cmp::Ordering::Equal => {
                    let (g, v) = self.next.take().unwrap();
                    return Ok(Some((g, serde_json::from_value(v["reply"].clone())?)));
                }
            }
        }
    }
}
pub fn playback(reader: impl BufRead, mut out: impl Write) -> Result<()> {
    let mut player = Playback::new(reader);
    while let Some((g, v)) = player.read()? {
        serde_json::to_writer(
            &mut out,
            &json!({"type":"recorded_interpretation","semantic":g,"reply":v["reply"],"requests":v["requests"],"writer_drops":v["writer_drops"]}),
        )?;
        out.write_all(b"\n")?;
    }
    Ok(())
}
