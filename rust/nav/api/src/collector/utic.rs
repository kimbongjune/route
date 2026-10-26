//! 경찰청 도시교통정보센터(UTIC) 돌발정보 `imsOpenData.do`(XML). 레코드마다 돌발 링크(`lineLinkId`, 없으면 `linkId`)를
//! 국가교통정보센터 돌발 이벤트와 같은 `IncidentItem`으로 바꿔 같은 규칙으로 통제 배율을 정한다.
//! 차선(`lane`)에 "전체차로"가 있으면 전면 통제, 그 밖의 차선 값이 있으면 부분 통제이다.
//! 코드 뜻은 UTIC 코드정의서 Incident_Code_20260630.xlsx를 따른다

use std::time::Duration;

use super::incident::IncidentItem;

const UTIC_URL: &str = "http://www.utic.go.kr/guide/imsOpenData.do";

/// UTIC 돌발 한 건
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UticRecord {
    pub id: String,
    /// 돌발유형 1 사고, 2 공사, 3 행사, 4 기상, 5 통제, 6 재난, 9 기타
    pub kind: String,
    pub title: String,
    pub lane: String,
    /// 통제정보 0 없음, 1 진행방향 통제, 2 양방향 통제
    pub control: String,
    pub links: Vec<u64>,
    pub lat: f64,
    pub lon: f64,
}

/// XML 문자 참조를 푼다
fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// 레코드 본문에서 태그 하나의 값. 없거나 "null"이면 빈 문자열이다
fn tag(record: &str, name: &str) -> String {
    let (open, close) = (format!("<{name}>"), format!("</{name}>"));
    let Some(start) = record.find(&open).map(|i| i + open.len()) else {
        return String::new();
    };
    let Some(len) = record[start..].find(&close) else {
        return String::new();
    };
    let value = unescape(record[start..start + len].trim());
    if value == "null" {
        String::new()
    } else {
        value
    }
}

/// 응답 XML을 레코드 목록으로 바꾼다. `<result>`가 없으면 오류이다
pub fn parse(xml: &str) -> Result<Vec<UticRecord>, String> {
    if !xml.contains("<result") {
        return Err(format!(
            "UTIC 돌발정보 응답이 결과 XML이 아님: {}",
            xml.chars().take(100).collect::<String>()
        ));
    }
    Ok(xml
        .split("<record>")
        .skip(1)
        .map(|r| {
            let r = r.split("</record>").next().unwrap_or(r);
            let ids = {
                let line = tag(r, "lineLinkId");
                if line.is_empty() {
                    tag(r, "linkId")
                } else {
                    line
                }
            };
            UticRecord {
                id: tag(r, "incidentId"),
                kind: tag(r, "incidenteTypeCd"),
                title: tag(r, "incidentTitle")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                lane: tag(r, "lane"),
                control: tag(r, "controlType"),
                links: ids
                    .split(',')
                    .filter_map(|v| v.trim().parse().ok())
                    .collect(),
                lat: tag(r, "locationDataY").parse().unwrap_or(0.0),
                lon: tag(r, "locationDataX").parse().unwrap_or(0.0),
            }
        })
        .collect())
}

/// 레코드를 링크마다 하나인 돌발 항목으로 바꾼다. 차선에 "전체차로"가 있으면 차단 통제 유형을 "전면"으로 한다
pub fn to_items(records: &[UticRecord]) -> Vec<IncidentItem> {
    records
        .iter()
        .flat_map(|r| {
            let full = r.lane.contains("전체차로");
            r.links.iter().map(move |link| IncidentItem {
                link_id: link.to_string(),
                lanes_block_type: if full {
                    "전면".to_string()
                } else {
                    String::new()
                },
                lanes_blocked: if full { String::new() } else { r.lane.clone() },
                message: r.title.clone(),
                ..Default::default()
            })
        })
        .collect()
}

/// 전국 UTIC 돌발정보를 한 번 요청한다
pub async fn fetch(http: &reqwest::Client, key: &str) -> Result<Vec<UticRecord>, String> {
    let text = http
        .get(UTIC_URL)
        .query(&[("key", key)])
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("UTIC 돌발정보 호출 실패: {}", e.without_url()))?
        .text()
        .await
        .map_err(|e| format!("UTIC 돌발정보 본문 읽기 실패: {}", e.without_url()))?;
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::super::incident::{Block, block_of};
    use super::*;

    #[test]
    fn parse_and_block() {
        let xml = r#"<?xml version="1.0" encoding="utf-8" ?>
<result>
<record><incidentId>A1</incidentId><incidenteTypeCd>2</incidenteTypeCd><linkId>1235035000</linkId>
<locationDataX>127.07</locationDataX><locationDataY>37.51</locationDataY>
<incidentTitle>[공사] 탄천동로   지하화 공사 &amp; 통제</incidentTitle><lane>양방향 전체차로</lane>
<lineLinkId>1235035000,1235028200</lineLinkId><controlType>2</controlType></record>
<record><incidentId>A2</incidentId><incidenteTypeCd>1</incidenteTypeCd><linkId>1180010201</linkId>
<locationDataX>126.92</locationDataX><locationDataY>37.50</locationDataY>
<incidentTitle>[사고] 여의대방로 2차로 고장 차량</incidentTitle><lane>2차로</lane>
<lineLinkId>1180010201</lineLinkId><controlType>0</controlType></record>
<record><incidentId>A3</incidentId><incidenteTypeCd>2</incidenteTypeCd><linkId>null</linkId>
<incidentTitle>[공사] 계족로</incidentTitle><lane>양방향 1,2차로</lane><lineLinkId>null</lineLinkId><controlType>2</controlType></record>
</result>"#;
        let records = parse(xml).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].title, "[공사] 탄천동로 지하화 공사 & 통제");
        assert_eq!(records[0].links, vec![1_235_035_000, 1_235_028_200]);
        assert!(records[2].links.is_empty());
        let items = to_items(&records);
        assert_eq!(items.len(), 3);
        assert_eq!(block_of(&items[0]), Block::Full);
        assert_eq!(block_of(&items[1]), Block::Full);
        assert_eq!(block_of(&items[2]), Block::Partial);
        assert!(parse("<html>error</html>").is_err());
    }
}
