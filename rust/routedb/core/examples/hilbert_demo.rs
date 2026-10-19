// 힐베르트 재번호화 동작 확인용 데모
use routedb_core::hilbert;

fn main() {
    println!("== 1. 4x4 격자의 힐베르트 순번 (위가 북쪽)");
    for y in (0..4).rev() {
        let row: Vec<String> = (0..4).map(|x| format!("{:>3}", hilbert::index(2, x, y))).collect();
        println!("  y={}  {}", y, row.join(""));
    }
    println!("  0 -> 1 -> 2 -> ... -> 15 순서로 따라가면 옆 칸으로만 이동한다");

    // shp 파일처럼 지도 위치와 상관없는 순서로 적힌 교차로 (이름, x, y)
    let nodes = [
        ("사", 2.0, 3.0), ("가", 0.0, 0.0), ("라", 3.0, 0.0), ("바", 1.0, 3.0),
        ("나", 1.0, 0.0), ("아", 3.0, 3.0), ("다", 2.0, 0.0), ("마", 0.0, 3.0),
    ];
    // 지도에서 바로 옆끼리 이어진 길
    let roads = [("가", "나"), ("나", "다"), ("다", "라"), ("마", "바"), ("바", "사"), ("사", "아")];

    println!("\n== 2. 지도");
    for y in (0..4).rev() {
        let row: Vec<String> = (0..4)
            .map(|x| nodes.iter().find(|n| n.1 == x as f64 && n.2 == y as f64).map_or(" .".to_string(), |n| n.0.to_string()))
            .collect();
        println!("  y={}  {}", y, row.join(" "));
    }
    println!("  길: 가-나-다-라 (아래 줄), 마-바-사-아 (위 줄)");

    println!("\n== 3. 내부 번호");
    let points: Vec<(f64, f64)> = nodes.iter().map(|n| (n.1, n.2)).collect();
    let order = hilbert::sort_order(&points);
    let shp_no = |name: &str| nodes.iter().position(|n| n.0 == name).unwrap();
    let hil_no = |name: &str| order.iter().position(|&i| nodes[i].0 == name).unwrap();
    println!("  shp 순서      {}", (0..nodes.len()).map(|i| format!("{}={}", nodes[i].0, i)).collect::<Vec<_>>().join(" "));
    println!("  힐베르트 순서 {}", order.iter().enumerate().map(|(k, &i)| format!("{}={}", nodes[i].0, k)).collect::<Vec<_>>().join(" "));

    println!("\n== 4. 길마다 양 끝 교차로의 번호 차이");
    let (mut sum_shp, mut sum_hil) = (0, 0);
    for (a, b) in roads {
        let gap_shp = shp_no(a).abs_diff(shp_no(b));
        let gap_hil = hil_no(a).abs_diff(hil_no(b));
        sum_shp += gap_shp;
        sum_hil += gap_hil;
        println!("  {}-{}  shp {}번-{}번 차이 {}   힐베르트 {}번-{}번 차이 {}", a, b, shp_no(a), shp_no(b), gap_shp, hil_no(a), hil_no(b), gap_hil);
    }
    println!("  합계  shp {}   힐베르트 {}", sum_shp, sum_hil);
}
