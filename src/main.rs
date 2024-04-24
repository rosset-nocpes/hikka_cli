use std::{
    collections::HashMap,
    env,
    error::Error,
    fs::{remove_file, File},
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    time::Duration,
};

use dialoguer::{theme::ColorfulTheme, Confirm, FuzzySelect, Input, Password, Select};
use dotenvy::dotenv;
use thirtyfour::{cookie::SameSite, prelude::*};
use tokio::time::sleep;

struct HikkaUser {
    username: String,
    moderator: bool,
    auth: bool,
    auth_token: String,
}

// TODO: change method of storing data
impl HikkaUser {
    async fn login(&mut self) -> Result<(), Box<dyn Error>> {
        let url = "https://hikka.io/anime?page=1&iPage=1";

        let mut gecko_spawner = Command::new("geckodriver")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let mut caps = DesiredCapabilities::firefox();

        // NOTE: Experimental
        caps.set_headless()?;

        let driver = WebDriver::new("http://localhost:4444", caps).await?;
        driver.goto(url).await?;

        // click login button
        driver
            .find(By::XPath("/html/body/nav/div[1]/div[2]/button[2]"))
            .await?
            .click()
            .await?;

        let email: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Email")
            .validate_with({
                let mut force = None;
                move |input: &String| -> Result<(), &str> {
                    if input.contains('@') || force.as_ref().map_or(false, |old| old == input) {
                        Ok(())
                    } else {
                        force = Some(input.clone());
                        Err("This is not a mail address; type the same value again to force use")
                    }
                }
            })
            .interact_text()
            .unwrap();

        let password = Password::with_theme(&ColorfulTheme::default())
            .with_prompt("Password")
            .interact()
            .unwrap();

        let client = reqwest::Client::new();

        // enter email
        driver
            .find(By::XPath(
                "/html/body/div[4]/div/div[2]/div[2]/form/div[1]/input",
            ))
            .await?
            .send_keys(email)
            .await?;

        // enter password
        driver
            .find(By::XPath(
                "/html/body/div[4]/div/div[2]/div[2]/form/div[2]/input",
            ))
            .await?
            .send_keys(password)
            .await?;

        driver.enter_frame(0).await?;

        if !driver
            .find_all(By::XPath("//*[@class='ctp-checkbox-label']/input"))
            .await?
            .is_empty()
        {
            driver
                .find(By::XPath("//*[@class='ctp-checkbox-label']/input"))
                .await?
                .click()
                .await?;

            // TODO: make dynamic check for success
            sleep(Duration::from_secs(1)).await;
        }

        driver.enter_default_frame().await?;

        // click login button in dialog
        driver
            .find(By::XPath(
                "/html/body/div[4]/div/div[2]/div[2]/form/div[4]/button[1]",
            ))
            .await?
            .click()
            .await?;

        sleep(Duration::from_secs(1)).await;

        // get auth token from cookie
        let auth_token = driver.get_named_cookie("auth").await?;

        let user: serde_json::Value = client
            .get("https://api.hikka.io/user/me")
            .header("auth", auth_token.value.clone())
            .send()
            .await?
            .json()
            .await?;

        gecko_spawner.kill().expect("not killed geckodriver!");

        let mut storage = File::create(".env")?;
        storage.write_fmt(format_args!(
            "USERNAME={}\nMODERATOR={}\nAUTH={}\nAUTH_TOKEN={}",
            user["username"].as_str().unwrap(),
            if user["role"] == "moderator" {
                "true"
            } else {
                "false"
            },
            "true",
            auth_token.value.clone().as_str()
        ))?;

        self.username = user["username"].as_str().unwrap().to_string();
        self.moderator = user["role"] == "moderator" || user["role"] == "admin";
        self.auth = true;
        self.auth_token = auth_token.value;

        Ok(())
    }

    fn logout(&mut self) -> Result<(), Box<dyn Error>> {
        remove_file(".env")?;

        self.username = "".to_string();
        self.moderator = false;
        self.auth = false;
        self.auth_token = "".to_string();

        Ok(())
    }
}

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

    let selections = &input;

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select anime")
        .default(0)
        .items(&selections[..])
        .interact()
        .unwrap();

    let mut title_arr: Vec<&str> = selections[selection].split(' ').collect();
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
async fn trans_char_anime_webdriver(slug: &str, user: &HikkaUser) -> Result<(), Box<dyn Error>> {
    if !user.auth {
        Err("Not authorized!")?;
    }

    // Creating url for anime characters
    let url = format!("https://api.hikka.io/anime/{}/characters", slug);

    let mut gecko_spawner = Command::new("geckodriver")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut auto = false;

    if user.moderator {
        let buf = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Do you want to auto approve edits?")
            .default(true)
            .interact()
            .unwrap();

        if buf {
            auto = true;
        }
    }

    let mut cookie = Cookie::new("auth", &user.auth_token); // TODO: store token in secure
    let mut caps = DesiredCapabilities::firefox();

    // NOTE: Experimental
    caps.set_headless()?;

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
                let name_input: String = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt(
                        format!(
                            "{} | {}",
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
                    )
                    .allow_empty(true)
                    .interact_text()
                    .unwrap();

                // if user enters button <ENTER>, skip characters
                if name_input.trim().is_empty() {
                    continue;
                }

                let elem_edits = driver.find(By::Tag("form")).await?;
                let name_elem = driver
                    .find(By::XPath("/html/body/main/div/div[1]/form/div[1]/div[1]"))
                    .await?;
                name_elem.click().await?;
                let button_elem = name_elem.find(By::XPath("div[2]/div/button[1]")).await?;
                button_elem.click().await?;
                let textname_elem = name_elem.find(By::XPath("div[2]/div[2]/input")).await?;
                textname_elem.send_keys(name_input).await?;

                let desc_elem = elem_edits.find(By::Tag("textarea")).await?;
                desc_elem.send_keys("Переклав ім'я (via HikkaCLI)").await?;

                // going into iframe of captcha
                driver.enter_frame(0).await?;

                if !driver
                    .find_all(By::XPath("//*[@class='ctp-checkbox-label']/input"))
                    .await?
                    .is_empty()
                {
                    driver
                        .find(By::XPath("//*[@class='ctp-checkbox-label']/input"))
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

// TODO: player, ani-cli
// fn player_integration(slug: &str, user: HikkaUser) -> Result<(), Box<dyn Error>> {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv()?;

    let env_file = File::open(".env").unwrap_or_else(|_| File::create(".env").unwrap());
    let buffered = BufReader::new(env_file);
    let line_count = buffered.lines().count();

    let mut user = if line_count == 4 {
        HikkaUser {
            username: env::var("USERNAME")?,
            moderator: env::var("MODERATOR")?.parse()?,
            auth: env::var("AUTH")?.parse()?,
            auth_token: env::var("AUTH_TOKEN")?,
        }
    } else {
        HikkaUser {
            username: "".to_string(),
            moderator: false,
            auth: false,
            auth_token: "".to_string(),
        }
    };

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        std::process::exit(1);
    });

    loop {
        let mut default_item = 0;
        let login_text = if user.auth {
            default_item = 1;
            format!("Logged in {} (Logout)", user.username)
        } else {
            "Login".to_string()
        };

        let _logged_user = format!("Logged in {} (Logout)", user.username).as_str();

        let selections = &[
            login_text.as_str(),
            "Translate characters from anime",
            "Search word in desc. (characters only)",
        ];

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Select option")
            .default(default_item)
            .items(&selections[..])
            .interact()
            .unwrap(); // TODO: make proper exit (with geckodriver kill)

        match selections[selection] {
            "Search word in desc. (characters only)" => {
                let word: String = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter a word")
                    .interact_text()
                    .unwrap();

                search_word_ch(word.trim()).await?;
            }
            "Translate characters from anime" => loop {
                let title: String = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter anime title")
                    .interact_text()
                    .unwrap();

                trans_char_anime_webdriver(&search_anime(title.trim()).await, &user).await?;
            },
            "Login" => {
                user.login().await?;
            }
            _logged_user => {
                user.logout()?;
            }
        }
    }
}
