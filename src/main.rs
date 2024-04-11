use std::{
    collections::HashMap,
    error::Error,
    io::{self, Write},
    process::{exit, Command, Stdio},
    time::Duration,
};

use dotenvy::dotenv;
use rustyline::{error::ReadlineError, DefaultEditor};
use serde_json::{Map, Value};
use thirtyfour::{cookie::SameSite, prelude::*};
use tokio::{io::AsyncBufReadExt, time::sleep};

// TODO: replace with fzf
async fn search_anime(anime: &str) -> String {
    let url = "https://api.hikka.io/anime?page=1&size=5";
    let body_data = format!(
        r#"{{"query": "{}", "sort": ["start_date:asc", "scored_by:desc"]}}"#,
        anime
    );

    let client = reqwest::Client::new();
    let response = client.post(url).body(body_data.to_owned()).send().await;
    let data: serde_json::Value = response.unwrap().json().await.unwrap();
    let list = data["list"].as_array().unwrap();

    for (i, element) in list.iter().enumerate() {
        println!(
            "{}. {} ({})",
            i + 1,
            element["title_ua"].as_str().unwrap(),
            element["year"]
        );
    }

    print!("Enter option: ");
    io::stdout().flush().unwrap();
    let mut opt = String::new();
    let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
    reader.read_line(&mut opt).await.unwrap();

    if opt == "\n" {
        exit(1);
    }

    let opt_alt: usize = opt.trim().parse().unwrap();
    let res = list[opt_alt - 1]["slug"].as_str().unwrap();
    res.to_string()
}

// TODO: Rewrite to search word in characters instead of edits
async fn search_word_ch(word: &str) -> Result<(), Box<dyn Error>> {
    let url = "https://api.hikka.io/edit/list?page=1&size=100";

    let mut map = HashMap::new();
    map.insert("content_type", "character");
    map.insert("status", "accepted");
    map.insert("slug", "");

    let client = reqwest::Client::new();
    let response = client.post(url).json(&map).send().await?;

    let data: serde_json::Value = response.json().await?;
    let pages = data["pagination"]["pages"].as_u64().unwrap();

    for page in 1..=pages {
        let url = format!("https://api.hikka.io/edit/list?page={}&size=100", page);

        let mut map = HashMap::new();
        map.insert("content_type", "character");
        map.insert("status", "accepted");
        map.insert("slug", "");

        let client = reqwest::Client::new();
        let response = client.post(url).json(&map).send().await?;

        if !response.status().is_success() {
            eprintln!(
                "Error fetching data from page {}: {}",
                page,
                response.status()
            );
            continue;
        }

        let data: serde_json::Value = response.json().await?;
        let list = data["list"].as_array().unwrap();

        for element in list {
            if !element["after"]["description_ua"].is_null() {
                match element["after"]["description_ua"].to_string().find(word) {
                    Some(_) => {
                        println!(
                            "{}: https://hikka.io/edit/{}",
                            element["content"]["name_en"].as_str().unwrap(),
                            element["edit_id"]
                        )
                    }
                    None => continue,
                }
            }
        }
    }
    Ok(())
}

// API Method
// NOTE: Don't working
#[allow(dead_code)]
async fn trans_char_anime(slug: &str) -> Result<(), Box<dyn Error>> {
    // Creating url for anime characters
    let url = format!("https://api.hikka.io/anime/{}/characters", slug);

    // INFO: Auth the user
    //
    // let mut credentials = HashMap::new();
    // credentials.insert("email", ""); // TODO: store credentials in secure
    // credentials.insert("password", "");
    let client = reqwest::Client::new();
    // client
    //     .post("https://api.hikka.io/auth/login")
    //     .json(&credentials)
    //     .header("captcha", "") // TODO: add captcha
    //     .send()
    //     .await?;

    // Create edit for every character
    let response = reqwest::get(url.clone()).await?;

    let data: serde_json::Value = response.json().await?;
    let pages = data["pagination"]["pages"].as_u64().unwrap();

    for page in 1..=pages {
        let url_p = format!("{}?page={}&size=100", url, page);
        let response = reqwest::get(url_p).await;

        let data: serde_json::Value = response.unwrap().json().await?;
        let list = data["list"].as_array().unwrap();

        for element in list {
            if !element["character"]["name_ua"].is_null() {
                // Output name_en & name_ja, then wait for input of name_ua
                // After all, create and approve edit
                print!(
                    "{} | {} : ",
                    element["character"]["name_en"].as_str().unwrap(),
                    element["character"]["name_ja"].as_str().unwrap()
                );
                io::stdout().flush().unwrap();

                let mut name_input = String::new();

                let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
                reader.read_line(&mut name_input).await?;

                let mut inner_map = Map::new();
                inner_map.insert(
                    "name_ua".to_string(),
                    Value::String(name_input.trim().to_string()),
                );

                let mut map = Map::new();
                map.insert(
                    "description".to_string(),
                    Value::String("Переклав ім'я (via HikkaCLI)".to_string()),
                );
                map.insert("auto".to_string(), Value::Bool(true));
                map.insert("after".to_string(), Value::Object(inner_map));

                client
                    .post(format!(
                        "https://api.hikka.io/edit/character/{}",
                        element["character"]["slug"]
                    ))
                    .json(&map)
                    .header("auth", "") // TODO: store auth in secure
                    .header("captcha", "") // TODO: add captcha
                    .send()
                    .await?;
                // TODO: check for success
            } else {
                continue;
            }
        }
    }

    Ok(())
}

// WebDriver method
// BUG: page of character edit not always opening and crashing tool
async fn trans_char_anime_webdriver(slug: &str) -> Result<(), Box<dyn Error>> {
    // Creating url for anime characters
    let url = format!("https://api.hikka.io/anime/{}/characters", slug);

    let mut gecko_spawner = Command::new("geckodriver")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    // INFO: Auth the user
    // TODO: First time launch login, after store token
    //
    // let mut credentials = HashMap::new();
    // credentials.insert("email", ""); // TODO: store credentials in secure
    // credentials.insert("password", "");
    let client = reqwest::Client::new();
    // client
    //     .post("https://api.hikka.io/auth/login")
    //     .json(&credentials)
    //     .header("captcha", "") // TODO: add captcha
    //     .send()
    //     .await?;
    let hikka_token = std::env::var("AUTH_TOKEN")?;
    let mut auto = false;

    let role_response: serde_json::Value = client
        .get("https://api.hikka.io/user/me")
        .header("auth", hikka_token.clone())
        .send()
        .await?
        .json()
        .await?;
    let role = role_response["role"].as_str().unwrap();

    if role == "moderator" {
        print!("Do you want to auto approve edits? [Y/n] ");
        io::stdout().flush().unwrap();

        let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
        let mut buf = String::new();
        reader.read_line(&mut buf).await?;

        match buf.trim() {
            "Y" | "y" if buf.is_empty() => auto = true,
            "N" | "n" => auto = false,
            _ => auto = true,
        }
    }

    let mut cookie = Cookie::new("auth", hikka_token); // TODO: store token in secure
    let mut caps = DesiredCapabilities::firefox();

    // NOTE: Experimental
    caps.add_arg("--headless")?;

    let driver = WebDriver::new("http://localhost:4444", caps).await?;

    // setting up cookie with auth token
    cookie.set_domain("hikka.io");
    cookie.set_path("/");
    cookie.set_same_site(SameSite::Lax);

    driver.goto("https://hikka.io").await?;
    driver.add_cookie(cookie.clone()).await?;

    // create edit for every character
    let response = reqwest::get(url.clone()).await?;

    let data: serde_json::Value = response.json().await?;
    let pages = data["pagination"]["pages"].as_u64().unwrap();

    for page in 1..=pages {
        let url_p = format!("{}?page={}&size=100", url, page);
        let response = reqwest::get(url_p).await;

        let data: serde_json::Value = response.unwrap().json().await?;
        let list = data["list"].as_array().unwrap();

        for element in list {
            if element["character"]["name_ua"].is_null() {
                // Output name_en & name_ja, then wait for input of name_ua
                // After all, create and approve edits

                // let suggest_name = wana_kana::to_kana();

                // going to character page
                driver
                    .goto(format!(
                        "https://hikka.io/edit/new?slug={}&content_type=character",
                        element["character"]["slug"].as_str().unwrap()
                    ))
                    .await?;

                let name_en = element["character"]["name_en"].to_owned();
                let name_ja = element["character"]["name_ja"].to_owned();
                // let waka_kana_ua = element["character"]["name_ja"].to_string().to_romaji();

                // creating interactive input & waiting for input
                // WARN: Experimental
                let mut rl = DefaultEditor::new()?;
                let name_input = rl.readline(
                    format!(
                        "{} | {} : ",
                        if !name_en.is_null() {
                            name_en.as_str().unwrap()
                        } else {
                            "-"
                        },
                        if !name_ja.is_null() {
                            name_ja.as_str().unwrap()

                        // TODO: integrate DeepL and 10ten
                        } else {
                            "-"
                        },
                        // if !name_ja.is_null() {
                        //     waka_kana_ua
                        // } else {
                        //     "-".to_string()
                        // }
                    )
                    .as_str(),
                );

                // if user enters button <ENTER>, skip character
                match name_input {
                    Ok(ref line) => {
                        if line.is_empty() {
                            continue;
                        }
                    }
                    Err(ReadlineError::Interrupted) => {
                        gecko_spawner.kill().expect("not killed!");
                    }
                    _ => todo!(),
                }

                // let mut inner_map = Map::new();
                // inner_map.insert(
                //     "name_ua".to_string(),
                //     Value::String(name_input.trim().to_string()),
                // );

                // let mut map = Map::new();
                // map.insert(
                //     "description".to_string(),
                //     Value::String("Переклав ім'я (via HikkaCLI)".to_string()),
                // );
                // map.insert("auto".to_string(), Value::Bool(true));
                // map.insert("after".to_string(), Value::Object(inner_map));

                let elem_edits = driver.find(By::Tag("form")).await?;
                let name_elem = driver
                    .find(By::XPath("/html/body/main/div/div[1]/form/div[1]/div[1]"))
                    .await?;
                name_elem.click().await?;
                let button_elem = driver
                    .find(By::XPath(
                        "/html/body/main/div/div[1]/form/div[1]/div[1]/div[2]/div/button[1]",
                    ))
                    .await?;
                button_elem.click().await?;
                let textname_elem = driver
                    .find(By::XPath(
                        "/html/body/main/div/div[1]/form/div[1]/div[1]/div[2]/div[2]/input",
                    ))
                    .await?;
                textname_elem.send_keys(name_input?).await?;

                let desc_elem = elem_edits.find(By::Tag("textarea")).await?;
                desc_elem.send_keys("Переклав ім'я (via HikkaCLI)").await?;

                // going into iframe of captcha
                driver.enter_frame(0).await?;

                if !driver
                    .find_all(By::XPath("/html/body/div/div/div[1]/div/label/input"))
                    .await?
                    .is_empty()
                {
                    driver
                        .find(By::XPath("/html/body/div/div/div[1]/div/label/input"))
                        .await?
                        .click()
                        .await?;

                    // TODO: make dynamic check for success
                    sleep(Duration::from_secs_f32(0.5)).await;
                }

                driver.enter_default_frame().await?;

                let send_elem = driver
                    .find(By::XPath(
                        "/html/body/main/div/div[1]/form/div[2]/div[2]/button[1]",
                    ))
                    .await?;

                let auto_send_elem = driver
                    .find(By::XPath(
                        "/html/body/main/div/div[1]/form/div[2]/div[2]/button[2]",
                    ))
                    .await?;

                if auto {
                    auto_send_elem.click().await?;
                } else {
                    send_elem.click().await?;
                }

                // TODO: Check if edit has been approved
            } else {
                continue;
            }
        }
    }

    // after all killing geckodriver for next iteration
    gecko_spawner.kill().expect("not killed!");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv()?;

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        std::process::exit(1);
    });

    println!("HikkaCLI | Tools to interact with hikka.io");

    print!("Options: \n1. Search word in desc (characters only) \n2. Translate characters from anime (WebDriver) \nEnter option: ");
    io::stdout().flush().unwrap();

    let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
    let mut option_app = String::new();
    reader.read_line(&mut option_app).await?;

    match option_app.trim() {
        "1" => {
            print!("Enter a word: ");
            io::stdout().flush().unwrap();
            let mut word = String::new();
            reader.read_line(&mut word).await?;

            search_word_ch(word.trim()).await?;
        }
        "2" => loop {
            print!("Enter anime title in ua: ");
            io::stdout().flush().unwrap();
            let mut title = String::new();
            reader.read_line(&mut title).await?;

            trans_char_anime_webdriver(&search_anime(title.trim()).await).await?;
        },
        &_ => todo!(),
    }

    Ok(())
}
