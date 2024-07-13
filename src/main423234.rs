
use std::io::{self, Write};

macro_rules! prntvar {
   ($vars: expr) => {
        print!("{}: ", stringify!($vars));
        print!("{:?}\n", $vars);
        let _ = io::stdout().flush();
    };
}



fn main() {
    let moin_leute = 1;
    let derWahreSigma = "jannis";
    
    prntvar!(derWahreSigma);
    prntvar!(moin_leute);
}