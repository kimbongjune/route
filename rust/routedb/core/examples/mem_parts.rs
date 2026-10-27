//! RouteDb를 여는 단계마다 프로세스 전용 메모리(PrivateUsage)가 얼마나 늘어나는지 출력한다(윈도우 전용)
//! cargo run --release -p routedb-core --example mem_parts -- <그래프 폴더>

use std::collections::HashMap;
use std::path::Path;

use cch::Cch;
use cch::bundle::{CchBundle, MetricBundle};
use routedb_core::file::GraphFile;
use routedb_core::linegraph;
use routedb_core::route::{RouteDb, WeightUpdate};
use routedb_core::snap::LinkIndex;

#[repr(C)]
#[derive(Default)]
struct Counters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set: usize,
    working_set: usize,
    quota_peak_paged: usize,
    quota_paged: usize,
    quota_peak_nonpaged: usize,
    quota_nonpaged: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_usage: usize,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
}

fn private_mb() -> f64 {
    let mut c = Counters {
        cb: size_of::<Counters>() as u32,
        ..Default::default()
    };
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    c.private_usage as f64 / 1e6
}

fn main() {
    let dir = std::env::args().nth(1).expect("그래프 폴더");
    let dir = Path::new(&dir);
    let mut last = private_mb();
    let mut step = |name: &str| {
        let now = private_mb();
        println!("{name}: +{:.0}MB (누계 {now:.0}MB)", now - last);
        last = now;
    };
    let file = GraphFile::open(&dir.join("moct.rdb")).unwrap();
    step("그래프 파일 mmap");
    let index = LinkIndex::build(file.geom_first(), file.geom_lat(), file.geom_lon());
    step(&format!(
        "도로 선분 R-tree(좌표 {}개)",
        file.geom_lat().len()
    ));
    let tail = linegraph::tails(file.first_out());
    step("간선 출발 교차로 배열");
    let bundle = CchBundle::open(&dir.join("moct.cch-struct")).unwrap();
    step("CCH 구조 mmap");
    let structure = Cch::load_struct(&dir.join("moct.cch-struct")).unwrap();
    step("CCH 구조 메모리 사본(customize용)");
    let metrics: Vec<MetricBundle> = [
        "moct.cch-metric",
        "moct.cch-metric-time",
        "moct.cch-metric-free",
    ]
    .iter()
    .map(|f| MetricBundle::open(&dir.join(f)).unwrap())
    .collect();
    step("가중치 파일 3개 mmap");
    let edge_of_link: HashMap<u64, u32> = file
        .link_id()
        .iter()
        .enumerate()
        .map(|(e, &id)| (id, e as u32))
        .collect();
    step("LINK_ID → 간선 HashMap");
    let factors = (
        vec![1.0f32; file.edge_count()],
        vec![1.0f32; file.edge_count()],
    );
    step("실시간 배율 배열 2개");
    drop((
        index,
        tail,
        bundle,
        structure,
        metrics,
        edge_of_link,
        factors,
        file,
    ));
    step("모두 해제");

    let db = RouteDb::open(
        &dir.join("moct.rdb"),
        &dir.join("moct.cch-struct"),
        [
            &dir.join("moct.cch-metric"),
            &dir.join("moct.cch-metric-time"),
            &dir.join("moct.cch-metric-free"),
        ],
    )
    .unwrap();
    step("RouteDb::open 전체");
    let link_id = db.file.link_id()[0];
    db.update_weights(&[WeightUpdate {
        link_id,
        speed_factor: Some(0.9),
        incident_factor: None,
    }]);
    step("실시간 배율 반영(최소시간·무료도로 customize 결과 메모리)");
}
