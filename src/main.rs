#![feature(proc_macro_hygiene, decl_macro)]
#[macro_use] extern crate rocket;
use anyhow::Result;
use webscreenshotlib::{screenshot_tab,OutputFormat};
use std::fs::{File,copy};
use std::io::prelude::*;
use rocket::response::{Response};
use rocket::http::Status;
use rocket::config::{Config, Environment};
use rocket::local::Client;
use rocket::http::{ContentType, Cookie};
use rocket::response::NamedFile;
use std::path::PathBuf;
use image::imageops::FilterType;
use image::ImageFormat;
use urlencoding::decode;
use serde::{Serialize, Deserialize};
use rocket::State;
use rocket::response::content;
use scraper::{Html,Selector,ElementRef};
use url::{Url, ParseError};
use std::time::Duration;
use reqwest::blocking::Client as RClient;
use rust_bert::pipelines::summarization::SummarizationModel;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use std::thread;
use std::sync::{Arc, Mutex};
use sled;
use std::collections::HashSet;



#[derive(Serialize, Deserialize)]
#[derive(Debug)]
enum DbKeys {
    SITE_CONTENT,
    SUMMARY,
}
#[derive(Serialize, Deserialize)]
#[derive(Debug)]
enum AuditState {
    NONE,
    IN_PROGRESS,
    DONE,
    TO_BE_REFRESHED
}


struct Cc {
    keywords: Selector,
    description:Selector,
    title:Selector,
    hs:Selector,
    links:Selector
}
#[derive(Serialize, Deserialize)]
#[derive(Debug)]
struct SiteContentEntry {
    key: DbKeys,
    state: AuditState,
    content: Vec<CcEntry>
}

impl Default for Cc {
    fn default() -> Self {
        Self {
            keywords:  Selector::parse(r#"meta[name="keywords"]"#).unwrap(),
            description: Selector::parse(r#"meta[name="description"]"#).unwrap(),
            title: Selector::parse(r#"title"#).unwrap(),
            hs: Selector::parse(r#"h1, h2, h3, h4, h5, h6"#).unwrap(),
            links: Selector::parse(r#"a"#).unwrap()
        }
    }
}

#[derive(Serialize, Deserialize)]
/// The ouput the scraper should eventually produce
#[derive(Debug)]
struct CcEntry {
    keywords: String,
    url: String,
    description: String,
    title: String,
    hs:String
}

// impl From<Html> for CcEntry{
//     fn from(doc:Html)->Self{

    
    
//     }
// }

pub trait Analyzer{
    fn analyze(&mut self,r:&SummarizationModel)->String;
}



pub trait Scraper: Sized {
    /// The type this scraper eventually produces
    type Output;
    fn scrape(
        &mut self,
        url: String,
    ) -> Vec<Self::Output>;

    fn scrape_single(
        &mut self,
        url: String,
    ) -> Self::Output;

    fn attrs(
        &mut self,
        doc:Html,
        select:Selector
    ) -> String;

    fn concat_all(
        &mut self, 
        doc: Html, 
        select: Selector
    )->String;

    fn get_links(
        &mut self,
        doc:Html,
        select:Selector
    ) -> Vec<String>;
}


impl Scraper for Cc {
    type Output = CcEntry;
       /// do your scraping
    fn scrape(
        &mut self,
        url: String
    ) -> Vec<Self::Output> {
        
        let mut  ccs:Vec<CcEntry>=Vec::new();

        let timeout = Duration::new(5, 0);
        let uri = Url::parse(&url).unwrap();
        let client = RClient::builder().user_agent("marlon").timeout(timeout).build().unwrap();
        let response = client.get(&url).header("Content-Type","text/html; charset=UTF-8").send().unwrap();
        let html = response.text().unwrap();
        //println!("html{:?}",html);
        let document = Html::parse_document(&html);
        
        let links = self.get_links(document.clone(),self.links.clone());
        println!("links :{:?}",links);

        let ou = CcEntry{
        //   .filter_map(|el| el.value().attr("id"))
            description:self.attrs(document.clone(), self.description.clone()),
            keywords: self.attrs(document.clone(), self.keywords.clone()),
            url: url.clone(),
            title: document.select(&self.title).next().unwrap().text().collect::<Vec<_>>().join(" "),
            hs: self.concat_all(document.clone(),self.hs.clone()),
        };

        ccs.push(ou);
        let mut a = 0;
        for link in links{
            if a<30 && (link.starts_with("/") || link.starts_with(&(uri.scheme().to_string()+"://"+uri.host_str().unwrap()))){
                println!("parsing {:?}",link);
                if(link.starts_with("/")){
                    let llink = uri.scheme().to_string()+"://"+uri.host_str().unwrap()+&link;
                    ccs.push(self.scrape_single(llink));
                }else{
                    ccs.push(self.scrape_single(link));
                }
                a+=1;
            }
            
        }
        ccs
        //Ok(None)>
    }

    fn scrape_single(
        &mut self,
        url: String)->Self::Output{

            let timeout = Duration::new(5, 0);
            let client = RClient::builder().user_agent("marlon").timeout(timeout).build().unwrap();
            let response = client.get(&url).header("Content-Type","text/html; charset=UTF-8").send().unwrap();
            let html = response.text().unwrap();
            //println!("html{:?}",html);
            let document = Html::parse_document(&html);

            let ou = CcEntry{
                //   .filter_map(|el| el.value().attr("id"))
                    description:self.attrs(document.clone(), self.description.clone()),
                    keywords: self.attrs(document.clone(), self.keywords.clone()),
                    url: url.clone(),
                    title: document.select(&self.title).next().unwrap().text().collect::<Vec<_>>().join(" "),
                    hs: self.concat_all(document.clone(),self.hs.clone()),
                };

            ou    
        }



    fn attrs(&mut self, doc: Html, select: Selector)->String{
        match doc.select(&select).next(){
            Some(r)=>r.value().attr("content").unwrap_or("").to_string(),
            None=>"".to_string()
        }
    }

    fn concat_all(&mut self, doc: Html, select: Selector)->String{
        doc.select(&select).map(|d|d.text().collect::<Vec<_>>().join(",")).collect::<Vec<_>>().join(",")
    }

    fn get_links(&mut self, doc: Html, select: Selector)->Vec<String>{
      doc.select(&select).map(|d|d.value().attr("href").unwrap_or("None").to_string()).collect::<Vec<String>>()
    }

}

impl Analyzer for CcEntry{
    fn analyze(&mut self,r:&SummarizationModel)->String{
        
        let buf = self.description.clone()+". "+&self.keywords+". "+&self.hs+". "+&self.title;
        let res = r.summarize(vec![buf.as_ref()]);
        res.join(". ")
    }   
}


impl Analyzer for Vec<CcEntry>{
    fn analyze(&mut self,r:&SummarizationModel)->String{
        let mut buf = String::new();
        for cc in self{
            buf+=&(cc.description.clone()+". "+&cc.keywords+". "+&cc.hs)
        }
        let res = r.summarize(vec![buf.as_ref()]);
        res.join(". ")
    }   
}

fn add_url(db:sled::Db,url:String)->HashSet<String>{
    let mut urls:HashSet<String> = HashSet::new();
    match db.get("urls_to_parse"){
        Ok(v)=>{
            match v{
                Some(res)=>{ 
                    let mut ress: HashSet<String> = bincode::deserialize(&res).unwrap();
                    ress.insert(url.clone());
                    let bts = bincode::serialize(&ress).unwrap();
                    db.insert("urls_to_parse",bts);
                    urls = ress;
                },
                None => {
                    let mut res:HashSet<String> = HashSet::new();
                    res.insert(url.clone());
                    let bts = bincode::serialize(&res).unwrap();
                    db.insert("urls_to_parse",bts);
                    urls = res;
                }
            }
        },
        Err(r) => {
            println!("Issue with {:?}",r);    
        }

    };
    urls
}


#[get("/audit/<refresh>/<url>")]
fn audit(middle: State<'_,middle>, refresh: bool, url: String) -> content::Json<String> {
    let url = decode(&url).unwrap();
    println!("url {:?}",url);
    let purl = Url::parse(&url).unwrap();
    println!("purl {:?}",purl);
    let urls = add_url(middle.db.clone(), url);
    // let mut scraper = Cc::default();
    // let mut ous = scraper.scrape(url);
    // let ret = middle.rarc.lock();
    // let keywords = match ret {
    //     Ok(r) => {
    //          let sum = &*r;
    //          ous.analyze(sum)
    //     },
    //     Err(e) => {
    //         println!("problem {:?}", e);
    //         "".to_string()
    //     }
    // };
 
    // let msg = format!("{{ \"content\": {:?} }}",keywords);
    
    // println!("{:?}",keywords);
    
    let msg = format!("{{ \"entries\": {:?} }}",urls);
    content::Json(msg)
}



#[get("/shot/<refresh>/<thumb>/<url>")]
fn shot(middle: State<'_,middle>,thumb: bool, refresh: bool, url: String) -> Option<NamedFile> {
    
    let durl = decode(&url).unwrap();
    let fname = url.replace(":", "_").replace("/","-").replace(".","_");
    println!("taking screenshot of {} thumb {}",url,thumb);
    
    let filename = PathBuf::from("data/".to_string()+&fname.clone()+".png");
    let filename_thumb = PathBuf::from("data/".to_string()+&fname.clone()+"_small.png");
    if !filename.exists() || !filename_thumb.exists() || refresh {        
        match screenshot_tab(&durl, OutputFormat::PNG,80,false,1280,1024,""){
            Ok(r)=>{
                println!("success {:?}",r.len());
                let mut file = File::create(filename.clone()).unwrap();
                file.write_all(&r).unwrap();
                //thumbnail
                let img = image::open(filename.clone()).unwrap();
                let scaled = img.resize(300, 400, FilterType::Lanczos3);
                let mut output = File::create(filename_thumb.clone()).unwrap();
                scaled.write_to(&mut output, ImageFormat::Png).unwrap()

            },
            Err(e)=>{
               
                copy("data/resources/not-found.png", "data/".to_string()+&fname.clone()+".png").unwrap();
                copy("data/resources/not-found_small.png", "data/".to_string()+&fname.clone()+"_small.png").unwrap();
                println!("failure {:?}",e);
            }
        }
    }
    if !thumb{
        println!("getting big");
        NamedFile::open(filename).ok()
    }else{
        NamedFile::open(filename_thumb).ok()
    }   
}

struct middle{
    db:sled::Db,
    rarc:Arc<Mutex<SummarizationModel>>
}


fn main() {
    let db = sled::open("data/db").unwrap();
    let r = SummarizationModel::new(Default::default()).unwrap();
    let rarc = Arc::new(Mutex::new(r));
    let dbt = Arc::new(Mutex::new(db.clone()));
    let _guard = thread::spawn(move || {
        let dbtp = &(*dbt.lock().unwrap());
        loop{    
            //checking db every 0.1 seconds
            match dbtp.get("urls_to_parse"){
                Ok(v)=>{
                    match v{
                        Some(res)=>{ 
                            let mut ress: HashSet<String> = bincode::deserialize(&res).unwrap();
                            for u in ress.drain(){
                                println!("{:?}",u);
                            }
                            let bts = bincode::serialize(&ress).unwrap();
                            dbtp.insert("urls_to_parse",bts);
                        },
                        None => {
                           
                        }
                    }
                },
                Err(r) => {
                    println!("Issue with {:?}",r);    
                }
        
            }      
            thread::sleep(Duration::from_millis(100));
        }
    });

    let config = Config::build(Environment::Staging)
    .read_timeout(30)
    .write_timeout(30)
    .finalize().unwrap();
    rocket::custom(config).manage(middle{db:db,rarc:rarc}).mount("/", routes![shot,audit]).launch();

    _guard.join();
}
