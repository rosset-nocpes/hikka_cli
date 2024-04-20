use std::{
    collections::HashMap,
    error::Error,
    io::{self, Cursor, Write},
    process::{Command, Stdio},
    time::Duration,
};

use dotenvy::dotenv;
use rustyline::{error::ReadlineError, DefaultEditor};
use serde_json::{Map, Value};
use skim::{prelude::*, Skim};
use thirtyfour::{cookie::SameSite, prelude::*};
use tokio::{io::AsyncBufReadExt, time::sleep};

// TODO: refactor & make dynamic search
async fn search_anime(anime: &str) -> String {
    let url = "https://api.hikka.io/anime?page=1&size=100";
    let body_data = format!(
        r#"{{"query": "{}", "sort": ["start_date:asc", "scored_by:desc"]}}"#,
        anime
    );

    let client = reqwest::Client::new();
    let response = client.post(url).body(body_data.to_owned()).send().await;
    let data: serde_json::Value = response.unwrap().json().await.unwrap();
    let list = data["list"].as_array().unwrap();

    // TODO: ignore case matching for ua language
    let options = SkimOptionsBuilder::default()
        .height(Some("100%"))
        .multi(true)
        .prompt(Some("Select anime: "))
        .build()
        .unwrap();

    let mut input: Vec<String> = Vec::new();

    for element in list {
        input.push(
            format!(
                "{} ({})",
                if !element["title_ua"].is_null() {
                    element["title_ua"].as_str().unwrap()
                } else if !element["title_en"].is_null() {
                    element["title_en"].as_str().unwrap()
                } else {
                    element["title_ja"].as_str().unwrap()
                },
                element["year"]
            )
            .to_string(),
        );
    }

    let final_input = input.join("\n");

    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(Cursor::new(final_input));

    let selected_items = Skim::run_with(&options, Some(items))
        .map(|out| out.selected_items)
        .unwrap_or_default();

    for item in selected_items.iter() {
        let item_col = item.output();

        let mut title_arr: Vec<&str> = item_col.split(' ').collect();
        title_arr.pop();
        let title = title_arr.join(" ");

        for element in list {
            let title_ua = if !element["title_ua"].is_null() {
                element["title_ua"].as_str().unwrap()
            } else {
                ""
            };
            let title_en = if !element["title_en"].is_null() {
                element["title_en"].as_str().unwrap()
            } else {
                ""
            };
            let title_ja = if !element["title_ja"].is_null() {
                element["title_ja"].as_str().unwrap()
            } else {
                ""
            };

            let slug = element["slug"].as_str().unwrap().to_string();

            if title == title_ua || title == title_en || title == title_ja {
                return slug;
            }
        }
    }

    "".to_string()
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
            "N" | "n" => (),
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

    let mut reader = tokio::io::BufReader::new(tokio::io::stdin());

    let options = SkimOptionsBuilder::default()
        .height(Some("100%"))
        .multi(true)
        .build()
        .unwrap();

    let input =
        "Translate characters from anime (WebDriver)\nSearch word in desc (characters only)"
            .to_string();

    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(Cursor::new(input));

    let selected_items = Skim::run_with(&options, Some(items))
        .map(|out| out.selected_items)
        .unwrap_or_default();

    for item in selected_items.iter() {
        match item.output() {
            Cow::Borrowed("Search word in desc (characters only)") => {
                print!("Enter a word: ");
                io::stdout().flush().unwrap();
                let mut word = String::new();
                reader.read_line(&mut word).await?;

                search_word_ch(word.trim()).await?;
            }
            Cow::Borrowed("Translate characters from anime (WebDriver)") => loop {
                print!("Enter anime title: ");
                io::stdout().flush().unwrap();
                let mut title = String::new();
                reader.read_line(&mut title).await?;

                trans_char_anime_webdriver(&search_anime(title.trim()).await).await?;
            },
            _ => todo!(),
        }
    }

    Ok(())
}
