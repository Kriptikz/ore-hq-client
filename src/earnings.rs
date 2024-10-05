use spl_token::amount_to_ui_amount;

use crate::database::AppDatabase;

pub fn earnings() {
    let app_db = AppDatabase::new();

    let daily_earnings = app_db.get_daily_earnings(7);

    for de in daily_earnings {
        println!("Day: {}, Total Earned: {} ORE", de.0, amount_to_ui_amount(de.1, ore_api::consts::TOKEN_DECIMALS));
    }
}
