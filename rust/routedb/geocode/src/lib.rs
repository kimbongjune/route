//! 지오코더. 도로명주소 전자지도의 건물과 국가관심지점정보 POI로 만든 색인 파일을 mmap으로 열어
//! 주소 검색, 장소 검색, 역지오코딩을 한다

pub mod index;
pub mod search;
pub mod text;

pub use index::{Address, Builder, GeocodeIndex, Poi, Region, Road};
pub use search::{Found, Hit, Kind};
