#![no_std]
#![feature(generic_associated_types)]

elrond_wasm::imports!();
elrond_wasm::derive_imports!();

#[derive(Clone, NestedEncode, NestedDecode, TopEncode, TopDecode, TypeAbi)]
pub struct Offer<M: ManagedTypeApi> {
    pub owner: ManagedAddress<M>,
    pub offered_token: EsdtTokenPayment<M>,
    pub wanted_token: EgldOrEsdtTokenPayment<M>,
}

#[derive(ManagedVecItem, Clone, NestedEncode, NestedDecode, TopEncode, TopDecode, TypeAbi)]
pub struct OfferOut<M: ManagedTypeApi> {
    pub index: u64,
    pub owner: ManagedAddress<M>,
    pub offered_token: EsdtTokenPayment<M>,
    pub wanted_token_identifier: EgldOrEsdtTokenIdentifier<M>,
    pub wanted_token_nonce: u64,
    pub wanted_token_amount: BigUint<M>,
}

#[elrond_wasm::contract]
pub trait HandShake {
    #[init]
    fn init(&self) {}

    #[only_owner]
    #[endpoint(setVault)]
    fn set_vault(&self, vault: ManagedAddress<Self::Api>) {
        self.vault().set(vault);
    }

    #[only_owner]
    #[endpoint(setFee)]
    fn set_fee(&self, fee: BigUint) {
        self.fee().set(fee);
    }

    #[only_owner]
    #[endpoint(pause)]
    fn pause(&self) {
        self.paused().set(true);
    }

    #[only_owner]
    #[endpoint(unpause)]
    fn unpause(&self) {
        self.paused().set(false);
    }

    #[endpoint(createOffer)]
    #[payable("*")]
    fn create_offer(
        &self,
        wanted_token_id: EgldOrEsdtTokenIdentifier,
        wanted_token_nonce: u64,
        wanted_token_amount: BigUint,
    ) {
        require!(!self.paused().get(), "Contract is paused");
        let token_offered = self.call_value().single_esdt();
        let caller = self.blockchain().get_caller();
        let sc = self.blockchain().get_sc_address();
        let nft_data = self.blockchain().get_esdt_token_data(
            &sc,
            &token_offered.token_identifier,
            token_offered.token_nonce,
        );
        require!(
            nft_data.token_type == EsdtTokenType::SemiFungible
                || nft_data.token_type == EsdtTokenType::NonFungible,
            "Token is not NFT/SFT"
        );

        let token_wanted =
            EgldOrEsdtTokenPayment::new(wanted_token_id, wanted_token_nonce, wanted_token_amount);
        let offer = Offer {
            owner: caller,
            offered_token: token_offered,
            wanted_token: token_wanted,
        };
        let index = self.create_offer_index();
        self.offers().insert(index, offer);
    }

    #[endpoint(cancelOffer)]
    fn cancel_offer(&self, index: u64) {
        require!(!self.paused().get(), "Contract is paused");
        let moffer = self.offers().get(&index);
        let caller = self.blockchain().get_caller();
        if let Some(offer) = moffer {
            require!(
                caller == offer.owner,
                "Only the owner of an offer can cancel it"
            );
            self.offers().remove(&index);
            self.send().direct_esdt(
                &offer.owner,
                &offer.offered_token.token_identifier,
                offer.offered_token.token_nonce,
                &(offer.offered_token.amount),
            );
            self.empty_offer_indexes().insert(index);
        } else {
            panic!("Offer not found");
        }
    }

    #[payable("*")]
    #[endpoint(acceptOffer)]
    fn accept_offer(&self, index: u64, royalties: BigUint) {
        require!(!self.paused().get(), "Contract is paused");
        let caller = self.blockchain().get_caller();
        let moffer = self.offers().get(&index);
        let offered_token = self.call_value().egld_or_single_esdt();
        if let Some(offer) = moffer {
            require!(caller != offer.owner, "You cannot accept your own offer");
            require!(offered_token == offer.wanted_token, "Token payment error");

            let sc = self.blockchain().get_sc_address();

            let nft_data = self.blockchain().get_esdt_token_data(
                &sc,
                &offer.offered_token.token_identifier,
                offer.offered_token.token_nonce,
            );

            let royal;
            if royalties > nft_data.royalties {
                royal = nft_data.royalties;
            } else {
                royal = royalties;
            }

            let dev_fee = &self.fee().get() * &offer.wanted_token.amount / &BigUint::from(10000u64);
            let crt_fee = &royal * &offer.wanted_token.amount / &BigUint::from(10000u64);
            let real_amount = &offer.wanted_token.amount - &dev_fee - &crt_fee;

            if dev_fee > BigUint::zero() {
                self.send().direct(
                    &self.vault().get(),
                    &offer.wanted_token.token_identifier,
                    offer.wanted_token.token_nonce,
                    &dev_fee,
                );
            }
            if crt_fee > BigUint::zero() {
                self.send().direct(
                    &nft_data.creator,
                    &offer.wanted_token.token_identifier,
                    offer.wanted_token.token_nonce,
                    &crt_fee,
                );
            }
            self.send().direct(
                &offer.owner,
                &offer.wanted_token.token_identifier,
                offer.wanted_token.token_nonce,
                &real_amount,
            );

            self.send().direct_esdt(
                &caller,
                &offer.offered_token.token_identifier,
                offer.offered_token.token_nonce,
                &offer.offered_token.amount,
            );

            self.offers().remove(&index);
            self.empty_offer_indexes().insert(index);
        } else {
            panic!("Offer not found");
        }
    }

    fn create_offer_index(&self) -> u64 {
        if self.empty_offer_indexes().is_empty() {
            let index = self.highest_offer_index().get().clone();
            self.highest_offer_index().set(index + 1);
            index
        } else {
            let index = self.empty_offer_indexes().get_by_index(1).clone();
            self.empty_offer_indexes().swap_remove(&index);
            index
        }
    }

    #[view(viewOffers)]
    fn view_offers(&self) -> ManagedVec<OfferOut<Self::Api>> {
        let mut offers = ManagedVec::new();
        for (index, offer) in self.offers().iter() {
            offers.push(OfferOut {
                index: index.clone(),
                owner: offer.owner.clone(),
                offered_token: offer.offered_token.clone(),
                wanted_token_identifier: offer.wanted_token.token_identifier,
                wanted_token_amount: offer.wanted_token.amount,
                wanted_token_nonce: offer.wanted_token.token_nonce,
            });
        }
        offers
    }

    #[view(viewOffer)]
    fn view_offer(&self, index: u64) -> Option<OfferOut<Self::Api>> {
        let offer = self.offers().get(&index);
        if let Some(offer) = offer {
            Some(OfferOut {
                index: index.clone(),
                owner: offer.owner.clone(),
                offered_token: offer.offered_token.clone(),
                wanted_token_identifier: offer.wanted_token.token_identifier,
                wanted_token_amount: offer.wanted_token.amount,
                wanted_token_nonce: offer.wanted_token.token_nonce,
            })
        } else {
            None
        }
    }

    //storage
    #[storage_mapper("offers")]
    fn offers(&self) -> MapMapper<u64, Offer<Self::Api>>;

    #[view(viewEmptyOfferIndexes)]
    #[storage_mapper("empty_offer_indexes")]
    fn empty_offer_indexes(&self) -> UnorderedSetMapper<u64>;

    #[view(viewHighestOfferIndex)]
    #[storage_mapper("highest_offer_index")]
    fn highest_offer_index(&self) -> SingleValueMapper<u64>;

    #[view(viewVault)]
    #[storage_mapper("vault")]
    fn vault(&self) -> SingleValueMapper<ManagedAddress>;

    #[view(viewFee)]
    #[storage_mapper("fee")]
    fn fee(&self) -> SingleValueMapper<BigUint>;

    #[view(viewIsPaused)]
    #[storage_mapper("pause")]
    fn paused(&self) -> SingleValueMapper<bool>;
}
