// CSR 동작 확인용 데모. 표준노드링크와 같은 모양의 가짜 데이터를 쓴다.
use std::collections::HashMap;

fn main() {
    // MOCT_NODE: NODE_ID, NODE_NAME
    let nodes: Vec<(u64, &str)> = vec![
        (1000000101, "가교차로"),
        (1000000201, "나교차로"),
        (1000000301, "다교차로"),
        (1000000401, "라교차로"),
        (1000000501, "마교차로"),
    ];
    // MOCT_LINK: LINK_ID, F_NODE, T_NODE. 양방향 도로는 ID가 100 차이 나는 링크 두 개다
    let links: Vec<(u64, u64, u64)> = vec![
        (1000001000, 1000000101, 1000000201), // 가 -> 나
        (1000001100, 1000000201, 1000000101), // 나 -> 가
        (1000002000, 1000000201, 1000000301), // 나 -> 다
        (1000002100, 1000000301, 1000000201), // 다 -> 나
        (1000003000, 1000000101, 1000000401), // 가 -> 라 (일방통행)
        (1000004000, 1000000401, 1000000301), // 라 -> 다 (일방통행)
        (1000005000, 1000000301, 1000000501), // 다 -> 마 (일방통행)
        (1000009000, 1000000501, 1000009999), // 마 -> 없는 노드
    ];
    let name: HashMap<u64, &str> = nodes.iter().cloned().collect();

    println!("== 1. NODE_ID -> 내부 번호");
    let mut index: HashMap<u64, u32> = HashMap::new();
    let mut node_id: Vec<u64> = Vec::new();
    for (i, (id, nm)) in nodes.iter().enumerate() {
        index.insert(*id, i as u32);
        node_id.push(*id);
        println!("  {} {} -> {}", id, nm, i);
    }

    println!("index: {:?}", index);
    println!("node_id: {:?}", node_id);

    println!("\n== 2. 링크를 내부 번호로 바꾸기");
    let mut edges: Vec<(u32, u32, u64)> = Vec::new();
    let mut skipped = 0;
    for &(link, f, t) in &links {
        match (index.get(&f), index.get(&t)) {
            (Some(&fi), Some(&ti)) => {
                println!("  {} {}({}) -> {}({})", link, name[&f], fi, name[&t], ti);
                edges.push((fi, ti, link));
            }
            _ => {
                println!("  {} 노드를 찾지 못해 건너뜀", link);
                skipped += 1;
            }
        }
    }

    println!("\n== 3. 출발 노드 순서로 정렬");
    edges.sort();
    for (e, (f, t, link)) in edges.iter().enumerate() {
        println!("  간선 {}: {} -> {}  (LINK {})", e, f, t, link);
    }

    println!("\n== 4. 노드별 나가는 간선 수 세고 누적합으로 first_out 만들기");
    let mut first_out = vec![0u32; node_id.len() + 1];
    for &(f, _, _) in &edges {
        first_out[f as usize + 1] += 1;
    }
    println!("  개수만 센 상태  {:?}", first_out);
    for v in 0..node_id.len() {
        first_out[v + 1] += first_out[v];
    }
    println!("  누적한 first_out {:?}", first_out);

    let head: Vec<u32> = edges.iter().map(|e| e.1).collect();
    let link_id: Vec<u64> = edges.iter().map(|e| e.2).collect();
    println!("  head            {:?}", head);
    println!("  link_id         {:?}", link_id);
    println!("  건너뛴 링크 {}", skipped);

    println!("\n== 5. 쓰기: 노드마다 바로 갈 수 있는 곳");
    for v in 0..node_id.len() {
        let (s, e) = (first_out[v] as usize, first_out[v + 1] as usize);
        let to: Vec<String> = (s..e)
            .map(|k| format!("{}(LINK {})", name[&node_id[head[k] as usize]], link_id[k]))
            .collect();
        println!(
            "  {} : 간선 {}..{} -> {}",
            name[&node_id[v]],
            s,
            e,
            if to.is_empty() {
                "없음".to_string()
            } else {
                to.join(", ")
            }
        );
    }
}
