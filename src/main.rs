use std::{collections::HashMap, io};
use clap::Parser;
use reqwest::blocking::get;
use scraper::{Html, Selector};
use regex::Regex;

#[derive(Parser, Debug)]
struct Args {
    /// Ratingupdate matchups url
    #[arg(short, long, default_value = "http://ratingupdate.info/matchups")]
    url: String,

    /// maximum iterations of the algorithm to run
    #[arg(short, long, default_value_t = 5000)]
    iters: usize,

    /// Wait until the final scores settle this much before giving a final answer
    #[arg(short, long, default_value_t = 0.000001f64)]
    max_settle: f64,

    /// cap on activation function used in the tierlist algorithm
    #[arg(short, long, default_value_t = 30f64)]
    activation_cap: f64,
    
    /// sort the rankings based on this matchup table
    #[arg(short, long, default_value_t = 0usize)]
    sort_by: usize,

    /// don't pause at the end
    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    no_pause: bool
}

#[derive(Debug)]
struct MatchupData {
    matchups: HashMap<String, HashMap<String, f64>>
}

fn get_matchups_from_ratingupdate(url: String) -> Vec<(Option<String>, MatchupData)> {
    let table_selector = Selector::parse("div.table-container").unwrap();
    let inner_table_selector = Selector::parse("table tbody").unwrap();
    let row_selector = Selector::parse("tr").unwrap();
    let cell_selector = Selector::parse("th,td span").unwrap();
    let cell_regex = Regex::new(r"(\d+(\.\d?))").unwrap();

    let response = get(url);
    let body = response.expect("Failed to fetch site content").text().expect("Failed to decode site content");
    let dom = Html::parse_document(&body);

    let mut matchup_sets = Vec::new();

    // temporary hack :(
    let header_selector = Selector::parse("h3").unwrap();
    let headers = dom.select(&header_selector).map(|elem| elem.inner_html()).collect::<Vec<String>>();

    for (j, outer_table) in dom.select(&table_selector).enumerate() {
        let mut characters: Vec<String> = Vec::new();
        let mut data: Vec<Vec<f64>> = Vec::new();

        // parse into intermediate format
        let table = outer_table.select(&inner_table_selector).next().unwrap();
        let mut row_iter = table.select(&row_selector).into_iter();
        row_iter.next();
        for row in row_iter {
            let mut cell_iter = row.select(&cell_selector).into_iter();
            let header_cell = cell_iter.next().unwrap();
            characters.push(header_cell.inner_html());
            let mut new_data_row: Vec<f64> = Vec::new();
            for cell in cell_iter {
                let content = cell.inner_html();
                let capture = cell_regex.captures(&content).unwrap().get(0).unwrap();
                let value: f64 = capture.as_str().parse().unwrap();
                new_data_row.push(value / 100f64);
            }
            data.push(new_data_row);
        }

        // parse into final format
        let mut matchups: HashMap<String, HashMap<String, f64>> = HashMap::new();

        for (i, char) in characters.iter().enumerate() {
            let mut matchup: HashMap<String, f64> = HashMap::new();
            for (j, match_value) in data[i].iter().enumerate() {
                if i != j {
                    matchup.insert(characters[j].clone(), *match_value);
                }
            }
            matchups.insert(char.clone(), matchup);
        }

        // find name
        // temporary hack :(
        let name = headers.get(j).map(String::clone);
        // idk how to get this to work :(
        // for sibling in outer_table.prev_siblings() {
        //     sibling.
        //     match sibling.value().as_element() {
        //         None => {},
        //         Some(elem) => if elem.name() == "h3" {
        //             println!("{:?}", elem);
        //             name = Some(elem.name().to_string());
        //             break;
        //         }
        //     }
        // }
        

        matchup_sets.push((name, MatchupData { matchups: matchups }));
    }

    matchup_sets
}

fn compute_tiers(data: &MatchupData, max_iters: usize, max_settle: f64, activation_cap: f64) -> (usize, f64, HashMap<String, f64>) {
    let mut scores: HashMap<String, f64> = HashMap::new();
    for char in data.matchups.keys() {
        scores.insert(char.clone(), 0f64);
    }
    let mut grand_mult = 1f64;
    let mut iters = 0usize;

    let ln2cap = activation_cap.log2();
    let activation = |x| activation_cap / (1f64 + 2f64.powf(ln2cap - x));

    loop {
        let mut new_scores: HashMap<String, f64> = HashMap::new();
        for char in scores.keys() {
            let sub_scores = data.matchups.get(char).unwrap().iter().map(|(opponent, score)| (2f64*score - 1f64) * activation(scores[opponent]) * grand_mult);
            new_scores.insert(char.clone(), sub_scores.sum());
        }
        scores = new_scores;
        let last_grand_mult = grand_mult;
        grand_mult = 1f64 / scores.values().map(|v| v.abs()).sum::<f64>();
        iters += 1;
        if (last_grand_mult - grand_mult).abs() < max_settle {
            break;
        } else if iters >= max_iters {
            break;
        } else if grand_mult.is_nan() {
            println!("Something went wrong!!");
            break;
        }
    }

    (iters, grand_mult, scores)
}

fn main() {
    let args = Args::parse();

    println!("Fetching matchup tables from {}", args.url);

    let matchup_sets = get_matchups_from_ratingupdate(args.url);

    println!("Computing tier lists for {} tables", matchup_sets.len());

    let tierlists = matchup_sets.into_iter().map(|(name, matchups)| {
        let (iters, grand_mult, tiers) = compute_tiers(&matchups, args.iters, args.max_settle, args.activation_cap);
        (name, iters, grand_mult, tiers)
    }).collect::<Vec<(Option<String>, usize, f64, HashMap<String, f64>)>>();

    println!("Compiling results");

    let first_tierlist = tierlists[args.sort_by].3.clone();
    let mut sorted = first_tierlist.into_iter().collect::<Vec<(String, f64)>>();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let mut score_table: Vec<(String, Vec<f64>)> = Vec::new();
    for (char, _) in sorted.iter() {
        score_table.push((char.clone(), tierlists.iter().map(|(_, _, _, matchups)| matchups[char]).collect()));
    }

    let widest = sorted.iter().map(|(char, _)| char.len()).max().unwrap() + 2;

    println!("Final results:");

    println!("");
    println!("{:width$}{}\n", "", tierlists.iter().map(|(name, _, _, _)| format!("{:>width$}", name.as_ref().unwrap_or(&"".to_string()), width=widest)).fold(String::new(), |a, b| a + &b), width=widest);
    println!("{:width$}{}\n", "Iters:", tierlists.iter().map(|(_, iters, _, _)| format!("{:>width$}", iters, width=widest)).fold(String::new(), |a, b| a + &b), width=widest);
    println!("{:width$}{}\n", "Grand mults:", tierlists.iter().map(|(_, _, mult, _)| format!("{:>width$}", format!("{:.4}", mult), width=widest)).fold(String::new(), |a, b| a + &b), width=widest);
    println!("Rankings:");
    for (char, scores) in score_table {
        println!("{:width$}{}", char, scores.iter().map(|score| format!("{:>width$}", format!("{:.4}", score), width=widest)).fold(String::new(), |a, b| a + &b), width=widest);
    }

    if !args.no_pause {
        println!("\nPress enter to exit.");
        let mut _buf = String::new();
        io::stdin().read_line(&mut _buf).unwrap();
    }
}
