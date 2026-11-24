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
        let f = File::create_new(path(xtv))?;
        let drops = Arc::new(AtomicU64::default());
        let q: Arc<Queue<Item>> = Queue::new(drops.clone());
        let work = q.clone();
        let lost = drops.clone();
        let join = thread::spawn(move || -> Result<()> {
            let mut out = BufWriter::new(f);
            let mut previous: Option<Arc<SemanticSceneGraph>> = None;
            let mut count = 0;
            let mut last_exchange = 0;
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
pub fn playback(reader: impl BufRead, mut out: impl Write) -> Result<()> {
    let mut last: Option<SemanticSceneGraph> = None;
    for line in reader.lines() {
        let v: serde_json::Value = serde_json::from_str(&line?)?;
        if v["schema"] != "xtion.semantic-sidecar/1" {
            return Err("unknown semantic sidecar version".into());
        }
        let record = &v["graph"];
        let g = if !record["snapshot"].is_null() {
            serde_json::from_value(record["snapshot"].clone())?
        } else {
            let mut g = last.take().ok_or("delta without initial snapshot")?;
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
            g.ns = record["ns"].as_u64().ok_or("missing timestamp")?;
            g.status = record["status"].as_str().ok_or("missing status")?.into();
            g.optics = serde_json::from_value(record["optics"].clone())?;
            g.relationships = delta.relationships;
            g.activities = serde_json::from_value(record["activities"].clone())?;
            g.expired = serde_json::from_value(record["expired"].clone())?;
            g
        };
        serde_json::to_writer(
            &mut out,
            &json!({"type":"recorded_interpretation","semantic":g,"reply":v["reply"],"requests":v["requests"],"writer_drops":v["writer_drops"]}),
        )?;
        out.write_all(b"\n")?;
        last = Some(g);
    }
    Ok(())
}
