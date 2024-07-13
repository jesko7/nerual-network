use reqwest::Client;
use serde_json::Value;
use base64;
use quartz_nbt;
use tokio;
use std::collections::HashMap;
use std::fmt::Debug;
use std::iter::zip;
use std::thread::panicking;
use std::fs;
use std::io::BufReader;
use std::time::{SystemTime, UNIX_EPOCH};
use std::io::Cursor;




#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Tier {
    Mythic,
    Legendary,
    Epic,
    Rare,
    Uncommon,
    Common,
    Special,
    VerySpecial,
    Divine,
    Ultimate
}


#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Filter {
    tier: Tier,
    name_id: String,
}



#[derive(Clone)]
struct Auction {
    claimed: bool,
    cost: i64,
    name: String,
    auction_id: String,
    start: i64,
    end: i64,
    item_bytes: String,
    base_tier: Tier,
    name_id: String,
    pet_level: Option<i32>,
    nbt_tree: quartz_nbt::NbtCompound,
    rarity_upgrades: i32
}
use std::any::type_name;


impl Debug for Auction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "name: {}, name id: {}, base tier: {:?}, petlvl: {:?}, bought: {}, cost: {}, rarity upgrades: {}, time since started: {}, time till end: {}, command: /viewauction {}", 
        self.name,
        self.name_id,
        self.base_tier,
        self.pet_level,
        self.claimed,
        self.cost,
        self.rarity_upgrades,
        compare_time_diff(self.start, SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64),
        compare_time_diff(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64, self.end),
        self.auction_id
    )
        
    }
}

static mut lowest_bin_keys: Vec<Filter> = vec![];
static mut lowest_bin_values: Vec<Vec<Auction>> = vec![];


fn type_of<T>(_: &T) -> &'static str {
    type_name::<T>()
}

impl Auction {
    fn new(claimed: bool, cost: i64, name: String, auction_id: String, start: i64, end: i64, item_bytes: String, tier: String) -> Self {
        //println!("bytes1: {}", item_bytes);
        let itambytes: Vec<u8> = base64::decode(&item_bytes).expect("could not decode item bytes");
        //println!("Decoded bytes length: {}", itambytes.len());
        let mut nbt_data: &[u8] = &itambytes;
        //println!("bytes2: {}, data: {:?}", item_bytes, nbt_data);
        
        let mut nbt_tree = quartz_nbt::io::read_nbt(&mut Cursor::new(nbt_data), quartz_nbt::io::Flavor::GzCompressed).expect("couldnt load nbt").0;
        let mut name_id = "none".to_string();
        let mut rarity_upgrades = 0;
        if let quartz_nbt::NbtTag::List(inner) = nbt_tree.inner_mut().get("i").unwrap() {
            if let quartz_nbt::NbtTag::Compound(inner) = inner.iter().next().unwrap() {
                if let quartz_nbt::NbtTag::Compound(inner) = inner.inner().get("tag").unwrap()  {
                    if let quartz_nbt::NbtTag::Compound(inner) = inner.inner().get("ExtraAttributes").unwrap()  {
                        if let quartz_nbt::NbtTag::String(inner) = inner.inner().get("id").unwrap()  {
                            name_id = inner.to_string();
                        }
                        if let Some(quartz_nbt::NbtTag::Int(inner)) = inner.inner().get("rarity_upgrades")  {
                            rarity_upgrades = *inner;
                        }
                    }
                }
            }
        }

        let mut base_tier = match tier.as_str() {
            "MYTHIC" => Tier::Mythic,
            "LEGENDARY" => Tier::Legendary,
            "EPIC" => Tier::Epic,
            "RARE" => Tier::Rare,
            "UNCOMMON" => Tier::Uncommon,
            "COMMON" => Tier::Common,
            "SPECIAL" => Tier::Special,
            "VERY_SPECIAL" => Tier::VerySpecial,
            "SUPREME" => Tier::Divine,
            "ULTIMATE" => Tier::Ultimate,
            other => panic!("not implemented: {}", other)
        };

        if rarity_upgrades == 1 {
            base_tier  = match base_tier {
                Tier::Uncommon => Tier::Common,
                Tier::Rare => Tier::Uncommon,
                Tier::Epic => Tier::Rare,
                Tier::Legendary => Tier::Epic,
                Tier::Mythic => Tier::Legendary,
                Tier::VerySpecial => Tier::Special,
                Tier::Divine => Tier::Mythic,
                other => panic!("not implemented: {:?}", other)
            }
        }
        

        //println!("extra attr: {:?}", nbt_tree);
        //println!("{:?}", nbt_tree);

        let mut auction = Auction {
            claimed,
            cost,
            name: name.clone(),
            auction_id,
            start,
            end,
            item_bytes,
            base_tier: base_tier.clone(),
            name_id: name_id.clone(),
            pet_level: None,
            nbt_tree,
            rarity_upgrades
        };




        if auction.name_id == "PET" {
            auction.name_id = auction.name.trim()[8..].trim().to_uppercase().replace(" ", "_");
            auction.pet_level = Some(auction.name[..8].trim().trim_start_matches("[Lvl").parse().unwrap_or(0));
        }



    

        let filter = Filter { tier: base_tier.clone(), name_id };

        unsafe {
            if let Some(index) = lowest_bin_keys.iter().position(|x| *x == filter) {
                lowest_bin_values[index].push(auction.clone());
                lowest_bin_values[index].sort_by_key(|auction| auction.cost);
            }
            else {
                lowest_bin_keys.push(filter);
                lowest_bin_values.push(vec![auction.clone()]);
            }
        }

        auction.clone()
    }

    fn get_value(&mut self, lowest_bins: &HashMap<Filter, Vec<Auction>>, bazaaritems: &HashMap<String, BazaarItem>, reforge_stones: &HashMap<String, ReforgeStone>) -> f64 {
        let mut enchants = None;
        let mut hpb = 0;

        let mut reforge = None;

        let mut stars = 0;

        if let quartz_nbt::NbtTag::List(inner) = self.nbt_tree.inner_mut().get("i").unwrap() {
            if let quartz_nbt::NbtTag::Compound(inner) = inner.iter().next().unwrap() {
                if let quartz_nbt::NbtTag::Compound(inner) = inner.inner().get("tag").unwrap()  {
                    if let quartz_nbt::NbtTag::Compound(inner) = inner.inner().get("ExtraAttributes").unwrap()  {
                        if let Some(quartz_nbt::NbtTag::Compound(inner)) = inner.inner().get("enchantments")  {
                            enchants = Some(inner);
                        }
                        if let Some(quartz_nbt::NbtTag::Int(inner)) = inner.inner().get("hot_potato_count")  {
                            hpb = *inner;
                        }
                        if let Some(quartz_nbt::NbtTag::String(inner)) = inner.inner().get("modifier")  {
                            reforge = Some(inner);
                        }
                        if let Some(quartz_nbt::NbtTag::Int(inner)) = inner.inner().get("upgrade_level")  {
                            stars = *inner;
                        }
                    } else {panic!()}
                } else {panic!()}
            } else {panic!()}
        } else {panic!()}

        let mut value: f64 = lowest_bins[&Filter{tier: self.base_tier.clone(), name_id: self.name_id.clone()}][0].cost.clone() as f64;

        let hot_potato_books = hpb.min(10);
        let fuming_potato_books = (hpb - hot_potato_books).max(0);

        let recombed = self.rarity_upgrades as f64;

        let normal_stars = stars.min(5);
        let master_stars = (stars - normal_stars).max(0);
        let mut cost: f64 = 0.;
        for master_star in 1..master_stars + 1 {
            cost += bazaaritems[
                match master_star {
                    1 => "FIRST_MASTER_STAR",
                    2 => "SECOND_MASTER_STAR",
                    3 => "THIRD_MASTER_STAR",
                    4 => "FOURTH_MASTER_STAR",
                    5 => "FIFTH_MASTER_STAR",
                    other => panic!()
                }
            ].insta_sell; 
        }
        println!("master stars: {}, cost: {}", master_stars, cost);
        value += cost;

        let recomb_cost = bazaaritems["RECOMBOBULATOR_3000"].insta_sell.clone();
        let hot_potato_book_cost = bazaaritems["HOT_POTATO_BOOK"].insta_sell.clone();
        let fuming_potato_book_cost = bazaaritems["FUMING_POTATO_BOOK"].insta_sell.clone();

        if let Some(enchants) = enchants {
            let enchs: HashMap<String, quartz_nbt::NbtTag> = quartz_nbt::NbtCompound::from(enchants.clone()).into_iter().collect();

            for ench_name in enchs.keys() {
                if let quartz_nbt::NbtTag::Int(lvl) = enchs[ench_name] {
                    let id = "ENCHANTMENT_".to_owned() + &ench_name.trim().to_uppercase().replace(" ", "_") + "_" + &lvl.to_string();

                    
                    let price;

                    if let Some(bazaar_item) = bazaaritems.get(&id) {
                        price = bazaar_item.insta_sell;
                    }
                    else if let Some(bazaar_item) = bazaaritems.get(&("ENCHANTMENT_ULTIMATE_".to_owned() + &ench_name.trim().to_uppercase().replace(" ", "_") + "_" + &lvl.to_string())) {
                        price = bazaar_item.insta_sell;
                    }
                    else {
                        price = 0.;
                    }
                    value += price;
                }
            }
        }

        if let Some(reforge_name) = reforge {
            if let Some(reforge_stone) = reforge_stones.get(reforge_name) {
                let cost = bazaaritems.get(&reforge_stone.reforge_stone_name_id).unwrap().insta_sell;

                let cost_to_apply = reforge_stone.cost_to_apply[
                    match self.base_tier {
                        Tier::Mythic => 5,
                        Tier::Legendary => 4,
                        Tier::Epic => 3,
                        Tier::Rare => 2,
                        Tier::Uncommon => 1,
                        Tier::Common => 0,
                        Tier::Special => 7,
                        Tier::VerySpecial => 8,
                        Tier::Divine => 6,
                        Tier::Ultimate => 9
                    }.min(reforge_stone.cost_to_apply.len() - 1)
                ];

                value += cost;
                value += cost_to_apply as f64;
            }
        }
        
        
        
        value += recomb_cost * recombed;
        value += hot_potato_book_cost * hot_potato_books as f64;
        value += fuming_potato_book_cost * fuming_potato_books as f64;

        //println!("lowest bin: {}, recomb: {}, hot_potato_book_cost: {}, fuming_potato_book_cost: {}", lowest_bins[&Filter{tier: self.base_tier.clone(), name_id: self.name_id.clone()}][0].cost.clone() as f64, recomb_cost * recombed, hot_potato_book_cost * hot_potato_books as f64, fuming_potato_book_cost * fuming_potato_books as f64);

        value
    }
}





fn compare_time_diff(time1: i64, time2: i64) -> i64 {
    let time1 = time1 / 1000;
    let time_diff = (time2 / 1000) - time1;
    time_diff
}

async fn get_auctions(client: &Client, data: &Value) -> Vec<Auction> {
    let mut auction_classes = Vec::new();

    let total_pages = data["totalPages"].as_i64().unwrap();
    for i in 0..total_pages {
        let response = client
            .get(&format!("https://api.hypixel.net/skyblock/auctions?page={}", i))
            .send()
            .await
            .expect("Failed to fetch data");
        let page_data: Value = response.json().await.expect("Failed to parse JSON");

        if let Some(auctions) = page_data["auctions"].as_array() {
            for auction in auctions {
                if !auction["bin"].as_bool().unwrap_or(false) || auction["claimed"].as_bool().unwrap() {
                    continue;
                }

                let auc = Auction::new(
                    auction["claimed"].as_bool().unwrap(),
                    auction["starting_bid"].as_i64().unwrap(),
                    auction["item_name"].as_str().unwrap().to_string(),
                    auction["uuid"].as_str().unwrap().to_string(),
                    auction["start"].as_i64().unwrap(),
                    auction["end"].as_i64().unwrap(),
                    auction["item_bytes"].as_str().unwrap().to_string(),
                    auction["tier"].as_str().unwrap().to_string()
                );

                auction_classes.push(auc);
            }
        }

        println!("got auction page {i}")
    }
    auction_classes
}


#[derive(Debug, Clone)]
struct BazaarItem {
    insta_buy: f64,
    insta_sell: f64,
    buy_volume: i64,
    sell_volume: i64,
    name_id: String,
    name: String,
}


fn get_bazaar_items(data: &Value) -> HashMap<String, BazaarItem> {
    let file = fs::File::open("src/items.json").unwrap();
    let reader = BufReader::new(file);
    let mut item_names: Value = serde_json::from_reader(reader).unwrap();
    let item_names  = item_names["items"].as_array_mut().unwrap().clone();

    let item_data = data["products"].clone();

    let mut items: HashMap<String, BazaarItem> = HashMap::new();
    
    for item_name in item_names {
        if let Value::Object(map) = item_name.clone() {
            let keys: Vec<&String> = map.keys().collect();
            let name_id_abstract = keys[0];
            

            let iteminfos = item_data[name_id_abstract].clone();

            let insta_buy = iteminfos ["quick_status"]["buyPrice"].clone().as_f64().unwrap();
            let insta_sell = iteminfos ["quick_status"]["sellPrice"].clone().as_f64().unwrap();

            let buy_volume = iteminfos ["quick_status"]["buyMovingWeek"].clone().as_i64().unwrap();
            let sell_volume = iteminfos ["quick_status"]["sellMovingWeek"].clone().as_i64().unwrap();

            let name_id = iteminfos ["product_id"].as_str().unwrap().to_string();
            let name = item_name.clone()[name_id_abstract].as_str().unwrap().to_string();


            items.insert(name_id.clone(), BazaarItem { insta_buy, insta_sell, buy_volume, sell_volume, name_id: name_id.clone(), name });


        } else {
            println!("not good");
        }
    }

    items
}
    

#[derive(Debug, Clone)]
struct ReforgeStone {
    cost_to_apply: Vec<i64>,
    reforge_stone_name_id: String,
    reforge_name: String,
}



#[tokio::main]
async fn main() {
    let client = Client::new();
    let response = client
        .get("https://api.hypixel.net/skyblock/auctions?page=0")
        .send()
        .await
        .expect("Failed to fetch data");

    let data: Value = response.json().await.expect("Failed to parse JSON");

    let total_auctions = data["totalAuctions"].as_i64().unwrap_or(0);
    let last_updated = data["lastUpdated"].as_i64().unwrap_or(0);

    let mut auctions = get_auctions(&client, &data).await;

    let mut lowest_bins: HashMap<Filter, Vec<Auction>> = HashMap::new();

    unsafe {
        for (filter, lowest_price) in zip(lowest_bin_keys.clone(), lowest_bin_values.clone()) {
            lowest_bins.insert(filter, lowest_price);
        }
    }
    
    let time_since_updated = compare_time_diff(last_updated, SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64);
    println!("Total auctions: {}, time since last updated: {}s", auctions.len(), time_since_updated);


    for name_id in lowest_bins.keys() {
        let lowest_bin_auctions = lowest_bins[name_id].clone();
        if lowest_bin_auctions.len() < 25 {
            continue;
        }
        let first_auc = lowest_bin_auctions[0].clone();
        let sec_auc = lowest_bin_auctions[1].clone();
        let profit = sec_auc.cost - first_auc.cost; 
        let profit_percent = (profit as f64 / sec_auc.cost as f64) * 100.;
        if profit > 500_000 && first_auc.cost < 12_000_000 {
            println!("snipe: /viewauction {}, profit: {}, profit%: {}, cost1: {}, cost2: {}", first_auc.auction_id, profit, profit_percent, first_auc.cost, sec_auc.cost);
        }
    }


    //GOT THE AUCTIONS NOW GETTING REFORGE_STONES-------------------------------------------------------------------------------------------


    let file = fs::File::open("src/reforges.json").unwrap();
    let reader = BufReader::new(file);
    let reforge_stones_json: Value = serde_json::from_reader(reader).unwrap();

    let reforge_stones_json = reforge_stones_json["reforges"].clone().as_array().unwrap().clone();

    let mut reforge_stones: HashMap<String, ReforgeStone> = HashMap::new();

    for reforge_stone_json in reforge_stones_json {
        if let Value::Object(inner) = reforge_stone_json {
            let reforge_name: String = inner.keys().collect::<Vec<&String>>()[0].to_string();

            let reforge_stone = ReforgeStone {
                reforge_name: reforge_name.to_lowercase().clone(),
                reforge_stone_name_id: inner.get(&reforge_name).unwrap().get("name_id").unwrap().as_str().unwrap().to_string(),
                cost_to_apply: inner
                .get(&reforge_name)
                .unwrap()
                .get("apply_cost")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|val| 
                    val
                    .as_str()
                    .unwrap()
                    .trim()
                    .replace(",", "")
                    .parse()
                    .unwrap()
                )
                .collect::<Vec<i64>>()
            };

            reforge_stones.insert(reforge_name.to_lowercase(), reforge_stone);
        }
    }
    //println!("reforge stones: {:#?}", reforge_stones);

    //GOT THE REFORGE_STONES NOW GETTING BAZZAR ITEMS---------------------------------------------------------------------------------------

    let client = Client::new();
    let response = client
        .get("https://api.hypixel.net/v2/skyblock/bazaar")
        .send()
        .await
        .expect("Failed to fetch data");

    let data: Value = response.json().await.expect("Failed to parse JSON");

    let last_updated = data["lastUpdated"].as_i64().unwrap_or(0);
    let time_since_updated = compare_time_diff(last_updated, SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64);

    println!("time since last updated: {}s", time_since_updated);

    let bazaar_items: HashMap<String, BazaarItem> = get_bazaar_items(&data);

    //println!("{:?}", bazaar_items);

    //GOT BAZZAR ITEMS GETTING FLIPS / SNIPES------------------------------------------------------------------------------------------------

    
    
    let mut items = lowest_bins[&Filter{tier: Tier::Legendary, name_id: "POWER_WITHER_CHESTPLATE".to_string()}].clone();
    for item in items.iter_mut() {
        println!("cost: {} value: {}, command: /viewauction {}", item.cost.clone(), item.get_value(&lowest_bins, &bazaar_items, &reforge_stones), item.auction_id);
    }


    for item in auctions.iter_mut() {
        if item.get_value(&lowest_bins, &bazaar_items, &reforge_stones) - item.cost as f64 > 3_000_000. {
            println!("cost: {} value: {}, command: /viewauction {}", item.cost.clone(), item.get_value(&lowest_bins, &bazaar_items, &reforge_stones), item.auction_id);
        }
    }
}
