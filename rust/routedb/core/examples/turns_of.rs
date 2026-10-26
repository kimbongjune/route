//! 그래프 파일에서 LINK_ID 간선의 나가는 통행(다음 링크와 경유 링크)을 출력한다
//! LINK_ID 대신 --dead를 주면 나가는 통행이 없는 간선 수를 출력하고, 다음 인자로 파일을 주면 경유 링크가 아닌 것의 LINK_ID 목록을 쓴다
//! cargo run --release -p routedb-core --example turns_of -- <moct.rdb> <LINK_ID> ...

use std::path::Path;

use routedb_core::file::GraphFile;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let file = GraphFile::open(Path::new(&args[1])).expect("그래프 파일을 열지 못함");
    let ids = file.link_id();
    if args.get(2).map(String::as_str) == Some("--dead") {
        let first = file.turn_first_out();
        let mut in_via = vec![false; ids.len()];
        for &v in file.via() {
            in_via[v as usize] = true;
        }
        let mut is_head = vec![false; ids.len()];
        for &h in file.turn_head() {
            is_head[h as usize] = true;
        }
        let dead: Vec<usize> = (0..ids.len()).filter(|&e| first[e] == first[e + 1]).collect();
        let via_only = dead.iter().filter(|&&e| in_via[e]).count();
        let head_too = dead.iter().filter(|&&e| !in_via[e] && is_head[e]).count();
        let isolated = dead.iter().filter(|&&e| !in_via[e] && !is_head[e]).count();
        println!("간선 {}개 중 나가는 통행이 없는 간선 {}개", ids.len(), dead.len());
        println!("  회전의 경유 링크로 쓰이는 것(교차로 그룹 내부 링크) {via_only}개");
        println!("  경유 링크가 아니고 다른 통행이 들어오는 것 {head_too}개");
        println!("  경유도 아니고 들어오는 통행도 없는 것 {isolated}개");
        let sample: Vec<u64> = dead.iter().filter(|&&e| !in_via[e] && is_head[e]).take(10).map(|&e| ids[e]).collect();
        println!("  들어오는 통행만 있는 간선 예: {sample:?}");
        if let Some(out) = args.get(3) {
            let lines: Vec<String> = dead
                .iter()
                .filter(|&&e| !in_via[e])
                .map(|&e| format!("{},{}", ids[e], if is_head[e] { "in" } else { "isolated" }))
                .collect();
            std::fs::write(out, lines.join("
")).expect("목록 저장 실패");
        }
        return;
    }
    for raw in &args[2..] {
        let id: u64 = raw.parse().expect("LINK_ID");
        let Some(e) = ids.iter().position(|&l| l == id) else {
            println!("{id}: 그래프에 없음");
            continue;
        };
        let first = file.turn_first_out();
        let heads: Vec<String> = (first[e] as usize..first[e + 1] as usize)
            .map(|t| {
                let via: Vec<u64> = file.via_of(t).iter().map(|&v| ids[v as usize]).collect();
                format!(
                    "{}{}",
                    ids[file.turn_head()[t] as usize],
                    if via.is_empty() {
                        String::new()
                    } else {
                        format!("(경유 {via:?})")
                    }
                )
            })
            .collect();
        println!(
            "{id}: 간선 {e}, 나가는 통행 {}개: {}",
            heads.len(),
            heads.join(", ")
        );
    }
}
