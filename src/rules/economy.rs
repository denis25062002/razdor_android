//! The economy as the original computes it (`docs/reference/original-mechanics/economy.md`):
//! wages and the noon payment, prices, market stock, barracks regrowth, daily healing,
//! village tribute and the villages' offers, victory gold.
//!
//! When the ticks happen (noon, midnight) is the clock's and `Game::pass_time`'s; this module
//! holds what happens then.

use serde::{Deserialize, Serialize};

use super::ai;
use super::content::{Bonus, HeroClass, ItemId, Nature, Source, WageKind};
use super::game::{Currency, Event, Game, Price, Tribute, PACK_SIZE};
use super::magic;
use super::units::{Stats, Unit};
use super::world::{Army, Location, LocationKind, Recruit};

/// Delphi's `Round`: halves go to the even neighbour (2.5 → 2, 3.5 → 4).
pub fn delphi_round(x: f64) -> i64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && (r as i64) % 2 != 0 {
        (r - x.signum()) as i64
    } else {
        r as i64
    }
}

/// `Round(num / den)` in integers, halves to even; `den` > 0.
pub fn round_ratio(num: i64, den: i64) -> i64 {
    let den = den.max(1);
    let (q, r) = (num.div_euclid(den), num.rem_euclid(den));
    match (2 * r).cmp(&den) {
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal => q + (q & 1),
    }
}

/// Market price factor in percent by the building's attitude −3..3 towards the buyer (own
/// buildings count as +3).
pub const RELATION_PERCENT: [i64; 7] = [170, 145, 125, 110, 100, 90, 75];

/// `Round(base × m)` with the relation factor m of `attitude` (+3 if the buyer owns it).
pub fn relation_price(base: i32, attitude: i8, own: bool) -> i32 {
    let a = if own { 3 } else { attitude.clamp(-3, 3) };
    round_ratio(base.max(0) as i64 * RELATION_PERCENT[(a + 3) as usize], 100) as i32
}

/// A `Merchant` in the army takes `price × 30 / 100` off a purchase.
pub fn merchant_price(price: i32) -> i32 {
    price - price * 30 / 100
}

/// Rear Service (`AddPayment`, Community): the whole gold bill ×178/256, or ×78/256 when the
/// player's stored income is exactly 0 (a quirk of the Community exe, kept as it is).
pub const REAR_SERVICE: (i32, i32) = (178, 78);

/// The gold bill as Rear Service cuts it (0xc250e4): the whole bill times 178 (or 78 when
/// the player's stored income is 0) div 256, truncated once.
pub fn rear_service(bill: i32, stored_income: i32) -> i32 {
    let k = if stored_income != 0 { REAR_SERVICE.0 } else { REAR_SERVICE.1 };
    bill * k / 256
}

/// Daily heals: a Medic in any army at midnight, the Ranger hero at noon and again when the
/// noon report is shown (percent of max HP).
pub const MEDIC_PERCENT: i32 = 10;
pub const RANGER_PERCENT: i32 = 15;
pub const RANGER_REPORT_PERCENT: i32 = 20;

/// Towns stock healing potions (these item ids) first, then maybe one of the others.
pub const TOWN_POTIONS: [u32; 3] = [98, 99, 100];
pub const TOWN_EXTRAS: [u32; 4] = [96, 97, 114, 115];
/// Market price window cap and the tries per random slot.
const PRICE_CAP: i32 = 5000;
const STOCK_TRIES: usize = 25;

/// Village offers: the priest casts this spell, the blessing one of these, furs are this item.
pub const PRIEST_SPELL: u32 = 1;
pub const BLESSING_SPELLS: [u32; 5] = [3, 5, 7, 9, 11];
pub const FURS_ITEM: u32 = 135;

/// Gold stock growth of any building with a maximum (0x4a1998): `income × √(1 − stock/max)`,
/// rounded, capped at the maximum. No maximum: no growth.
pub fn grow_stock(stock: i32, income: i32, max: i32) -> i32 {
    if max <= 0 {
        return stock;
    }
    let room = 1.0 - stock as f64 / max as f64;
    if room <= 0.0 {
        return stock;
    }
    (stock as i64 + delphi_round(income as f64 * room.sqrt())).clamp(0, max as i64) as i32
}

/// Mana stock growth: [`grow_stock`]'s rule, but the original adds on a byte, so a sum above
/// 255 wraps before the cap (only possible with a maximum near 255).
pub fn grow_mana(stock: i32, income: i32, max: i32) -> i32 {
    if max <= 0 {
        return stock;
    }
    let room = 1.0 - stock as f64 / max as f64;
    if room <= 0.0 {
        return stock;
    }
    let sum = (stock as i64 + delphi_round(income as f64 * room.sqrt())) as u8 as i32;
    sum.min(max)
}

/// The one thing a village may offer on a visit besides its tribute (economy.md §3), with the
/// original's numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum VillageOffer {
    /// 1: a long blessing, one of [`BLESSING_SPELLS`].
    Blessing,
    /// 2: the priest heals (spell [`PRIEST_SPELL`]).
    Priest,
    /// 3: furs, item [`FURS_ITEM`].
    Furs,
    /// 4: the witch gives 300–500 mana.
    Witch,
    /// 5: the innkeeper pays the army.
    Innkeeper,
}

/// What accepting an offer gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfferResult {
    Blessing(u32),
    Healed(i32),
    Furs(ItemId),
    Mana(i32),
    Paid(usize),
}

/// What the noon payment did.
pub(crate) struct NoonPay {
    pub mana_wages: i32,
    pub unpaid: usize,
    pub deserted: Vec<super::content::UnitId>,
}

/// One barracks slot at midnight: +1 when `Random(days div max)` is 0, so with chance
/// `1 / (days div max)`, always when that divisor is 0 or 1. Every slot with a unit, a
/// maximum and room draws once, whatever the divisor (the stream counts the draw).
/// `roll(n)` is the original's `Random(n)`.
pub fn regrow(r: &mut Recruit, days: i32, roll: &mut dyn FnMut(i32) -> i32) {
    let Some(stock) = r.stock.as_mut() else { return };
    if *stock >= r.max || r.max <= 0 {
        return;
    }
    if roll(days / r.max) == 0 {
        *stock += 1;
    }
}

impl Game {
    /// The difficulty factor F: 100 with "impossible difficulty", else 120.
    pub fn difficulty(&self) -> i32 {
        self.content.options.difficulty_factor.max(1)
    }

    /// Wage of unit `u` for a day, in its currency: kind 1 by the cost brackets, kind 2
    /// `Cost / CostMercenaryDiv`, kinds 0 and 3 nothing. A kind-1 elemental is paid in mana
    /// (Community). This is the full wage: Rear Service cuts only the whole bill at noon.
    pub fn unit_wage(&self, u: &Unit) -> Price {
        let w = self.content.wage_for(u.def, u.wage_kind);
        if u.wage_kind == WageKind::Recruit && self.content.paid_in_mana(u.def) {
            Price { amount: w, currency: Currency::Mana }
        } else {
            Price::gold(w)
        }
    }

    /// Daily wage of squad member `i` (the hero is free), in its currency; a corpse is not
    /// billed.
    pub fn wage(&self, i: usize) -> i32 {
        self.squad.get(i).filter(|u| u.alive()).map_or(0, |u| self.unit_wage(u).amount)
    }

    /// The army's gold and mana bills (0x4a16d4): the wages of its living units; a corpse
    /// goes to the resurrection bill instead. Garrisons are never paid.
    fn bills(&self) -> (i32, i32) {
        let (mut gold, mut mana) = (0, 0);
        for u in self.squad.iter().filter(|u| u.alive()) {
            let p = self.unit_wage(u);
            match p.currency {
                Currency::Gold => gold += p.amount,
                Currency::Mana => mana += p.amount,
            }
        }
        (gold, mana)
    }

    /// The gold wage bill, as the hire tab and the noon report show it (no Rear Service).
    pub fn daily_wages(&self) -> i32 {
        self.bills().0
    }

    /// Mana wages due at the next noon (elementals).
    pub fn daily_mana_wages(&self) -> i32 {
        self.bills().1
    }

    /// The income the hire tab and the noon report show (0x497308): the `income` of every
    /// town, castle and fort of the player, each ×F/100. Towns are counted, though they pay
    /// nothing at noon, and the stocks actually paid are not.
    pub fn daily_income(&self) -> i32 {
        let f = self.difficulty();
        let kinds = [LocationKind::Town, LocationKind::Castle, LocationKind::Fort];
        self.world.locations.iter().filter(|l| l.owned() && kinds.contains(&l.kind)).map(|l| l.gold_income * f / 100).sum()
    }

    /// The player's noon payment (economy.md §1, 0x4a41d8): his castles and forts pay their
    /// gold stock ×F/100 and the villages linked to his buildings theirs (no mana, no towns);
    /// the stored income becomes the castles' and forts' `income` plus the village stocks.
    /// Then the gold bill goes out (cut by Rear Service) and the mana bill out of his mana;
    /// mana at 0 or below raises the sticky mana-short flag. With the gold not below 0
    /// everyone's last pay is now. Else the cheapest paid units get their full wage back
    /// and go unpaid until it is not, the gold is set to 0, and every unit last paid more
    /// than `MaxTimeNotUpkeep` ago leaves with its worn items.
    pub(crate) fn pay_noon(&mut self) -> NoonPay {
        let now = self.clock.total_minutes() as u64;
        let f = self.difficulty();
        let (mut income, mut stored) = (0, 0); // the player's base income is 0
        let n = self.world.locations.len();
        for l in 0..n {
            let w = &mut self.world.locations;
            if w[l].owned() && w[l].kind.capturable() {
                income += w[l].tribute_gold * f / 100;
                stored += w[l].gold_income;
                w[l].tribute_gold = 0;
            }
            if w[l].kind == LocationKind::Village && w[l].linked.is_some_and(|k| w.get(k).is_some_and(Location::owned)) {
                income += w[l].tribute_gold;
                stored += w[l].tribute_gold;
                w[l].tribute_gold = 0;
            }
        }
        self.gold += income;
        self.stored_income = stored;
        let (bill, mana_bill) = self.bills();
        // Rear Service tests the stored income just rewritten.
        self.gold -= if self.squad_has_any(&Bonus::AddPayment) { rear_service(bill, self.stored_income) } else { bill };
        self.pay_mana_bill(mana_bill);
        let c = self.content.clone();
        let elemental = |u: &Unit| c.paid_in_mana(u.def);
        let short = self.gold < 0;
        let mut deserted = Vec::new();
        if !short {
            for u in self.squad.iter_mut() {
                u.last_paid = now;
                // The original's slip (0xc25f7d): with the mana-short flag up, the elementals
                // go unpaid and every other unit keeps its old mark, so a unit left unpaid by an
                // earlier short noon stays unpaid though its wage was paid. The flag stays up.
                if !self.mana_short {
                    u.unpaid = false;
                } else if elemental(u) {
                    u.unpaid = true;
                }
            }
        } else {
            for u in self.squad.iter_mut() {
                u.unpaid = false;
            }
            loop {
                // The paid unit of kind 1 or 2, not an elemental, with the lowest full wage
                // (the earliest of equals); a corpse too, though it was not billed.
                let pick = (0..self.squad.len())
                    .filter(|&i| {
                        let u = &self.squad[i];
                        !u.unpaid && u.wage_kind.is_paid() && !elemental(u)
                    })
                    .min_by_key(|&i| (c.wage_for(self.squad[i].def, self.squad[i].wage_kind), i));
                if self.mana_short {
                    // Slots 1–11 only, as the original's loop.
                    for u in self.squad.iter_mut().take(11).filter(|u| elemental(u)) {
                        u.unpaid = true;
                    }
                    self.mana_short = false;
                }
                let Some(i) = pick else {
                    self.gold += 100_000;
                    break;
                };
                let w = c.wage_for(self.squad[i].def, self.squad[i].wage_kind);
                self.squad[i].unpaid = true;
                self.gold += w;
                if self.gold >= 0 {
                    break;
                }
            }
            for u in self.squad.iter_mut().filter(|u| !u.unpaid) {
                u.last_paid = now;
            }
            self.gold = 0;
            // Deserters leave with their worn items (Army_RemoveUnit moves nothing to the
            // pack). The hero is always paid, so he never leaves.
            let limit = c.options.max_time_not_upkeep.max(0) as u64;
            let mut i = 0;
            while i < self.squad.len() {
                if self.squad[i].last_paid + limit < now {
                    deserted.push(self.squad.remove(i).def);
                } else {
                    i += 1;
                }
            }
        }
        let unpaid = self.squad.iter().filter(|u| u.unpaid).count();
        // As every army's wage payment, the hero's ends by marking his pairs with the AI's
        // armies to be rescored (0x4a26e8).
        self.mark_dirty(super::ai::HERO);
        NoonPay { mana_wages: mana_bill, unpaid, deserted }
    }

    /// The mana bill of any army's noon (Community, 0xc25eef) comes out of the *player's*
    /// mana; at 0 or below it is set to 0 and the mana-short flag goes up (also with a bill
    /// of 0, so whenever the player has no mana).
    pub(crate) fn pay_mana_bill(&mut self, bill: i32) {
        self.mana -= bill;
        if self.mana <= 0 {
            self.mana = 0;
            self.mana_short = true;
        }
    }

    /// Any unit of the player's army, dead or alive, has bonus `b` (the original reads the
    /// unit slots' bonus bytes without an HP test: Rear Service, Medic).
    pub(crate) fn squad_has_any(&self, b: &Bonus) -> bool {
        self.squad.iter().any(|u| u.stats(&self.content).has(b))
    }

    /// The Ranger hero heals every wounded unit of his army `pct`% (truncated): 15 at noon,
    /// 20 more when the noon report is shown.
    pub(crate) fn ranger_heal(&mut self, pct: i32) {
        if self.hero_class() == Some(HeroClass::Ranger) {
            let c = self.content.clone();
            for u in self.squad.iter_mut() {
                heal_percent(&c, u, pct);
            }
        }
    }

    /// Midnight for every building and army (0x4a1998): markets draw new random goods,
    /// barracks regrow by chance, the gold and mana stocks of every building with a maximum
    /// grow, garrisons heal `GarrisonAutoHeal`% and every army with a Medic (dead or alive)
    /// [`MEDIC_PERCENT`]%. The draws go building by building, as in the original (engine.md
    /// §3.4): its market restock first, then its barracks slots.
    pub(crate) fn economy_midnight(&mut self) {
        let days = self.content.options.max_day_count_for_new_unit;
        for l in 0..self.world.locations.len() {
            self.restock_market(l);
            let Game { world, rng, .. } = self;
            let loc = &mut world.locations[l];
            for r in loc.recruits.iter_mut() {
                regrow(r, days, &mut |n| rng.random(n));
            }
            loc.tribute_gold = grow_stock(loc.tribute_gold, loc.gold_income, loc.gold_max);
            loc.tribute_mana = grow_mana(loc.tribute_mana, loc.mana_income, loc.mana_max);
        }
        let c = self.content.clone();
        let garrison = c.options.garrison_auto_heal;
        for l in self.world.locations.iter_mut() {
            for s in l.stationed.iter_mut() {
                heal_percent(&c, &mut s.unit, garrison);
            }
            for t in l.garrison.iter_mut() {
                heal_troop(&c, t, garrison);
            }
        }
        if self.squad_has_any(&Bonus::ArmyMedic) {
            for u in self.squad.iter_mut() {
                heal_percent(&c, u, MEDIC_PERCENT);
            }
        }
        for a in self.world.armies.iter_mut() {
            if army_has(&c, a, &Bonus::ArmyMedic) {
                for t in a.troops.iter_mut() {
                    heal_troop(&c, t, MEDIC_PERCENT);
                }
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Prices
    // ---------------------------------------------------------------------------------------

    /// Price to buy `item` here: its cost times the building's relation factor
    /// ([`relation_price`]); a `Merchant` in the army takes 30% off.
    pub fn buy_price(&self, item: ItemId) -> i32 {
        let (attitude, own) = self.location.map_or((3, true), |l| {
            let l = &self.world.locations[l];
            (l.attitude, l.owned())
        });
        let p = relation_price(self.content.item(item).cost, attitude, own);
        if self.squad_has(&Bonus::Merchant) {
            merchant_price(p)
        } else {
            p
        }
    }

    /// Price a market pays for `item`: `Cost × ItemSaleCost × F / 10000`, a `Merchant` adds
    /// half of that again. The relation does not count.
    pub fn sell_price(&self, item: ItemId) -> i32 {
        let cost = self.content.item(item).cost.max(0) as i64;
        let p = (cost * self.content.options.item_sale_cost as i64 * self.difficulty() as i64 / 10_000) as i32;
        if self.squad_has(&Bonus::Merchant) {
            p + p / 2
        } else {
            p
        }
    }

    /// Only items worth more than 1 can be sold (personal items have a negative price).
    pub fn can_sell(&self, item: ItemId) -> bool {
        self.content.try_item(item).is_some_and(|d| d.cost > 1)
    }

    /// Every shop: the fixed goods not sold yet, plus random goods drawn anew (economy.md §2):
    /// as many as the building's count minus the fixed goods; towns first get healing
    /// potions; the rest are market items priced within the window, at most twice the same
    /// (once when the window's top is above 500); then all sorted by price.
    pub(crate) fn restock_markets(&mut self) {
        for l in 0..self.world.locations.len() {
            self.restock_market(l);
        }
    }

    /// The restock of building `l` (nothing when it has no shop).
    pub(crate) fn restock_market(&mut self, l: usize) {
        let market = self.content.items_from(Source::Market);
        let demo = self.world.demo;
        let kind = self.world.locations[l].kind;
        let Some(shop) = &self.world.locations[l].shop else { return };
        let (count, (lo, hi)) = (shop.random, shop.price);
        let fixed = shop.fixed.clone();
        let mut left = count.saturating_sub(if demo { 0 } else { fixed.len() });
        let (lo, hi) = if hi <= 0 && lo <= 0 {
            (i32::MIN, i32::MAX)
        } else {
            let mut hi = hi.min(PRICE_CAP);
            if self.rng.random(5) == 0 {
                hi += 1;
            }
            (lo.max(5).min(hi), hi)
        };
        let exists = |id: u32| self.content.try_item(ItemId(id)).is_some();
        let mut random: Vec<ItemId> = Vec::new();
        if kind == LocationKind::Town && !demo {
            let potions: Vec<u32> = TOWN_POTIONS.into_iter().filter(|&i| exists(i)).collect();
            if !potions.is_empty() {
                for _ in 0..(count / 5 + 1).min(left) {
                    random.push(ItemId(potions[self.rng.random(potions.len() as i32) as usize]));
                    left -= 1;
                }
            }
            let extras: Vec<u32> = TOWN_EXTRAS.into_iter().filter(|&i| exists(i)).collect();
            if left > 6 && !extras.is_empty() && self.rng.random(5) != 0 {
                random.push(ItemId(extras[self.rng.random(extras.len() as i32) as usize]));
                left -= 1;
            }
        }
        let pool: Vec<ItemId> = market.iter().copied().filter(|&i| (lo..=hi).contains(&self.content.item(i).cost)).collect();
        let most = if hi > 500 { 1 } else { 2 };
        for _ in 0..left {
            if pool.is_empty() {
                break;
            }
            for _ in 0..STOCK_TRIES {
                let i = pool[self.rng.random(pool.len() as i32) as usize];
                if random.iter().filter(|&&x| x == i).count() < most {
                    random.push(i);
                    break;
                }
            }
        }
        let mut stock = fixed;
        stock.extend(random);
        stock.retain(|&i| self.content.try_item(i).is_some());
        stock.sort_by_key(|&i| self.content.item(i).cost);
        if let Some(shop) = &mut self.world.locations[l].shop {
            shop.stock = stock;
        }
    }

    // ---------------------------------------------------------------------------------------
    // Healing and resurrection
    // ---------------------------------------------------------------------------------------

    /// Price to heal squad member `i` fully:
    /// `max(1, Round(missing / max × Cost × HealingConst% × 100 / F))`, in mana for
    /// elementals. `None` if it is dead or unhurt.
    pub fn heal_price(&self, i: usize) -> Option<Price> {
        let u = self.squad.get(i)?;
        let max = u.max_hp(&self.content);
        if !u.alive() || u.hp >= max {
            return None;
        }
        let cost = self.content.unit(u.def).cost.max(0) as i64;
        let pct = self.content.options.healing_const.max(0) as i64;
        let missing = (max - u.hp) as i64;
        let amount = round_ratio(missing * cost * pct, max as i64 * self.difficulty() as i64).max(1);
        Some(Price::for_unit(&self.content, u.def, amount as i32))
    }

    /// Price to resurrect squad member `i`: `Round(Cost × ResurectConst% × 100 / F)`. `None`
    /// if it is alive or past the window.
    pub fn resurrect_price(&self, i: usize) -> Option<Price> {
        self.resurrection_minutes_left(i)?;
        let u = &self.squad[i];
        let cost = self.content.unit(u.def).cost.max(0) as i64;
        let amount = round_ratio(cost * self.content.options.resurect_const.max(0) as i64, self.difficulty() as i64);
        Some(Price::for_unit(&self.content, u.def, amount as i32))
    }

    // ---------------------------------------------------------------------------------------
    // Villages
    // ---------------------------------------------------------------------------------------

    /// The village here, if it pays: not ill-disposed, with something waiting, and the hero
    /// not a Rogue (a Rogue gets nothing from villages).
    fn village_ready(&self) -> Option<usize> {
        let l = self.location?;
        let v = &self.world.locations[l];
        let rogue = self.content.unit(self.hero().def).nature == Nature::Rogue;
        (v.kind == LocationKind::Village && (v.tribute_gold > 0 || v.tribute_mana > 0) && !v.hostile() && !rogue).then_some(l)
    }

    /// Tribute (gold) the village here would pay now, if any is waiting.
    pub fn tribute_available(&self) -> Option<i32> {
        self.village_ready().map(|l| self.world.locations[l].tribute_gold)
    }

    /// Empties the village's gold and mana stock: (gold, mana).
    fn empty_village(&mut self, l: usize) -> (i32, i32) {
        let v = &mut self.world.locations[l];
        let got = (v.tribute_gold, v.tribute_mana);
        (v.tribute_gold, v.tribute_mana) = (0, 0);
        got
    }

    /// Takes all the waiting gold and mana (no difficulty factor). In the demo a village
    /// sometimes pays with an item instead of the gold. Taking the tribute lets this village
    /// make an offer again.
    pub fn collect_tribute(&mut self) -> Option<Tribute> {
        let l = self.village_ready()?;
        let (gold, mana) = self.empty_village(l);
        self.mana += mana;
        if self.offered_at == Some(l) {
            self.offered_at = None;
        }
        if self.offer.is_some_and(|(k, _)| k == l) {
            self.offer = None;
        }
        if self.world.demo && self.pack.len() < PACK_SIZE && self.rng.range(1, 100) <= super::game::TRIBUTE_ITEM_CHANCE {
            if let Some(item) = self.roll_item(Source::Tribute) {
                self.pack.push(item);
                return Some(Tribute::Item(item));
            }
        }
        self.gold += gold;
        Some(Tribute::Gold(gold))
    }

    /// The offer the village here makes on this visit, if any.
    pub fn village_offer(&self) -> Option<VillageOffer> {
        self.offer.filter(|&(l, _)| Some(l) == self.location).map(|(_, o)| o)
    }

    /// Good (own-army) spells in effect, counted per unit.
    fn good_spells_on_units(&self) -> usize {
        let now = self.clock.total_minutes() as u64;
        let units = self.squad.iter().filter(|u| u.alive()).count();
        self.effects
            .iter()
            .filter(|e| e.lasts_at(now) && self.spell(e.spell).is_some_and(|s| !magic::targets_enemy(s)))
            .map(|e| if e.leader { 1 } else { units })
            .sum()
    }

    /// Whether offer `o` could be made now (its conditions, not its roll).
    fn offer_fits(&self, o: VillageOffer) -> bool {
        let c = &self.content;
        let n = self.squad.len();
        match o {
            VillageOffer::Innkeeper => {
                let unpaid = self.squad.iter().filter(|u| u.unpaid).count();
                unpaid >= n / 2 && self.gold - self.daily_wages() + self.stored_income < 0
            }
            VillageOffer::Priest => {
                // The original's second test counts the *living* units (HP above 0), not the
                // wounded ones, against the army size div 2 (0x4bba40; kept).
                let living: Vec<&Unit> = self.squad.iter().filter(|u| u.alive()).collect();
                let missing: i32 = living.iter().map(|u| u.max_hp(c) - u.hp).sum();
                self.spell(PRIEST_SPELL).is_some() && missing > 50 && living.len() >= n / 2
            }
            VillageOffer::Blessing => BLESSING_SPELLS.iter().any(|&s| self.spell(s).is_some()) && self.good_spells_on_units() <= n,
            VillageOffer::Furs => {
                let furs = ItemId(FURS_ITEM);
                c.try_item(furs).is_some() && self.pack.len() < 25 && self.pack.iter().filter(|&&i| i == furs).count() <= 2
            }
            VillageOffer::Witch => self.mana < self.gold && self.spells.len() >= 3 && self.good_spells_on_units() == 0,
        }
    }

    /// Entering building `l`: a village whose gold stock is above 0 and which did not make
    /// the last offer rolls for one (economy.md §3, 0x4bba40): innkeeper 1/2, priest 1/3,
    /// then blessing, furs and witch 1/6 each, in that order, each with its conditions; the
    /// first that passes is offered. Every roll is drawn until one passes, also for the kind
    /// offered last time, which cannot pass. "Last" then becomes what was offered, or none
    /// when nothing was, so a kind can come back after an empty visit.
    pub(crate) fn visit_village(&mut self, l: usize) {
        self.offer = None;
        let v = &self.world.locations[l];
        if v.kind != LocationKind::Village || v.tribute_gold <= 0 || self.offered_at == Some(l) || self.village_ready() != Some(l) {
            return;
        }
        use VillageOffer::*;
        let mut picked = None;
        for (o, n) in [(Innkeeper, 2), (Priest, 3), (Blessing, 6), (Furs, 6), (Witch, 6)] {
            if self.rng.random(n) == 0 && self.offer_fits(o) && self.last_offer != Some(o) {
                picked = Some(o);
                break;
            }
        }
        self.last_offer = picked;
        if let Some(o) = picked {
            self.offer = Some((l, o));
            self.offered_at = Some(l);
        }
    }

    /// Entering village `l`: unless it made an offer (answered first, [`Game::accept_offer`] or
    /// [`Game::decline_offer`]), the hero takes the tribute at once (economy.md §3, 0x4c6000;
    /// the footage's "tribute already collected").
    pub(crate) fn auto_tribute(&mut self, l: usize) -> Option<Event> {
        if self.village_offer().is_some() || self.village_ready() != Some(l) {
            return None;
        }
        let mana = self.world.locations[l].tribute_mana;
        self.collect_tribute().map(|paid| Event::Tribute { at: l, paid, mana })
    }

    /// Declines the village's offer and takes its tribute instead.
    pub fn decline_offer(&mut self) -> Option<Tribute> {
        self.village_offer()?;
        self.offer = None;
        self.collect_tribute()
    }

    /// Accepts the village's offer: it takes effect and empties both of the village's stocks.
    pub fn accept_offer(&mut self) -> Option<OfferResult> {
        let o = self.village_offer()?;
        let l = self.location?;
        self.offer = None;
        self.empty_village(l);
        let now = self.clock.total_minutes() as u64;
        Some(match o {
            VillageOffer::Innkeeper => {
                for u in self.squad.iter_mut() {
                    u.unpaid = false;
                    u.last_paid = now;
                }
                OfferResult::Paid(self.squad.len())
            }
            VillageOffer::Priest => {
                let spell = self.spell(PRIEST_SPELL)?.clone();
                OfferResult::Healed(self.apply_spell_to_army_ext(&spell, true))
            }
            VillageOffer::Blessing => {
                let known: Vec<u32> = BLESSING_SPELLS.into_iter().filter(|&s| self.spell(s).is_some()).collect();
                let id = known[self.rng.random(known.len() as i32) as usize];
                let spell = self.spell(id)?.clone();
                self.apply_spell_to_army_ext(&spell, true);
                OfferResult::Blessing(id)
            }
            VillageOffer::Furs => {
                self.pack.push(ItemId(FURS_ITEM));
                OfferResult::Furs(ItemId(FURS_ITEM))
            }
            VillageOffer::Witch => {
                let mana = 300 + 50 * self.rng.random(5);
                self.mana += mana;
                OfferResult::Mana(mana)
            }
        })
    }

    // ---------------------------------------------------------------------------------------
    // Loot
    // ---------------------------------------------------------------------------------------

    /// Gold an AI army takes from a beaten one carrying `gold`: all of it below
    /// `MinVictoryGold`, else `gold / VictoryGoldDiv` (the minimum is a threshold, not a floor).
    pub fn victory_gold(&self, gold: i32) -> i32 {
        let o = &self.content.options;
        let gold = gold.max(0);
        if gold < o.min_victory_gold {
            gold
        } else {
            gold / o.victory_gold_div.max(1)
        }
    }

    /// Gold the player takes from a beaten army: `gold / VictoryGoldDiv` (no minimum), plus
    /// its daily wage total unless it is a peasant army or its units carry no money
    /// *(guess: which flag the exe reads there is not decoded)*.
    pub fn player_victory_gold(&self, a: &Army) -> (i32, i32) {
        let gold = a.gold.max(0) / self.content.options.victory_gold_div.max(1);
        let wages = if a.ai.no_money || a.ai.style == ai::Style::Peasant { 0 } else { ai::army_wages(&self.content, &a.troops) };
        (gold, wages)
    }
}

/// Heals a wounded (living, hurt) unit by `pct`% of its max HP.
fn heal_percent(c: &super::content::Content, u: &mut Unit, pct: i32) {
    let max = u.max_hp(c);
    if u.alive() && u.hp < max && pct > 0 {
        u.hp = (u.hp + max * pct / 100).min(max);
    }
}

/// Heals a wounded AI troop by `pct`% of its max HP.
fn heal_troop(c: &super::content::Content, t: &mut super::world::Troop, pct: i32) {
    if t.alive() && t.hurt > 0 && pct > 0 {
        let max = super::ai::troop_max_hp(c, t);
        t.hurt = (t.hurt - max * pct / 100).max(0);
    }
}

fn army_has(c: &super::content::Content, a: &Army, b: &Bonus) -> bool {
    a.troops.iter().any(|t| Stats::of_level(c, t.unit, t.level.max(1)).has(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::rng::Rng;

    #[test]
    fn delphi_rounding_goes_to_even() {
        assert_eq!([0.5, 1.5, 2.5, 3.5, 2.4, 2.6, -2.5].map(delphi_round), [0, 2, 2, 4, 2, 3, -2]);
        assert_eq!([(5, 2), (7, 2), (9, 4), (11, 4), (1, 3)].map(|(n, d)| round_ratio(n, d)), [2, 4, 2, 3, 0]);
    }

    #[test]
    fn relation_factor_table() {
        let p: Vec<i32> = (-3..=3).map(|a| relation_price(1000, a, false)).collect();
        assert_eq!(p, [1700, 1450, 1250, 1100, 1000, 900, 750]);
        assert_eq!(relation_price(1000, -3, true), 750, "own = +3");
        assert_eq!(relation_price(10, 0, false), 11);
        assert_eq!(merchant_price(1450), 1015);
    }

    #[test]
    fn village_stock_grows_by_the_square_root_rule() {
        // 30 a day, max 90: 30 × √(2/3) = 24.49 → 24, 30 × √(0.4) = 18.97 → 19, ...
        let mut s = 30;
        let mut seen = vec![s];
        for _ in 0..4 {
            s = grow_stock(s, 30, 90);
            seen.push(s);
        }
        assert_eq!(seen, [30, 54, 73, 86, 90], "slower than 30 a day");
        assert_eq!(grow_stock(90, 30, 90), 90);
        assert_eq!(grow_stock(10, 30, 0), 10, "no maximum: no growth");
    }

    #[test]
    fn mana_stock_grows_on_a_byte() {
        // 200 + Round(255 × √(1 − 200/255)) = 200 + 118 = 318, which wraps to 62 (0x4a1998).
        assert_eq!(grow_mana(200, 255, 255), 62);
        assert_eq!(grow_mana(5, 5, 20), 9, "5 × √0.75 = 4.33");
        assert_eq!(grow_mana(3, 5, 0), 3, "no maximum: no growth");
        assert_eq!(rear_service(59, 10), 41);
        assert_eq!(rear_service(59, 0), 17);
    }

    #[test]
    fn barracks_grow_by_chance() {
        let mut r = Recruit::new(super::super::content::UnitId(1), 0, 5);
        // 10 div 5 = 2: a roll of 0 (in 0..2) grows it.
        regrow(&mut r, 10, &mut |n| {
            assert_eq!(n, 2);
            1
        });
        assert_eq!(r.stock, Some(0));
        regrow(&mut r, 10, &mut |_| 0);
        assert_eq!(r.stock, Some(1));
        let mut one = Recruit::new(super::super::content::UnitId(1), 0, 1);
        regrow(&mut one, 10, &mut |n| {
            assert_eq!(n, 10, "1 in 10 a day");
            0
        });
        assert_eq!(one.stock, Some(1));
        let mut full = Recruit::new(super::super::content::UnitId(1), 1, 1);
        regrow(&mut full, 10, &mut |_| panic!("a full slot does not draw"));
    }

    #[test]
    fn a_barracks_slot_draws_even_when_it_must_grow() {
        // 10 div 20 = 0 and 10 div 10 = 1: Random(0) and Random(1) are 0, but they are
        // drawn.
        let mut drawn = Vec::new();
        for max in [20, 10] {
            let mut r = Recruit::new(super::super::content::UnitId(1), 0, max);
            let mut rng = Rng::new(1);
            regrow(&mut r, 10, &mut |n| {
                drawn.push(n);
                rng.random(n)
            });
            assert_eq!(r.stock, Some(1));
            let mut once = Rng::new(1);
            once.random(0);
            assert_eq!(rng.state(), once.state(), "one draw");
        }
        assert_eq!(drawn, [0, 1]);
    }
}
