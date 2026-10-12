# SolNet Admin Walkthrough — Video Scripts & Shooting Notes

Narration for an admin-led tour of the app. Conventions: **bold** = say it,
`mono` = on-screen label, *italics* = stage direction, never narrated.
Each page block runs ~30–45 seconds spoken. Prep and per-take checklists included.

## Shoot prep (do once)

- [ ] Freighter installed, connected with the **admin wallet**
  (`GB7CM4…6BG5W`). Every admin lock in the video depends on this.
- [ ] Network set to **testnet** in Network settings; all 12 contract IDs configured.
- [ ] Test owner wallet address handy for the slash demo:
  `GDYXYYV63URVSWQMYRNG4DXXH3HUQOFXA2ZYSXDORCZFADJ72XRRSF6W`
  (must hold a `deposit_collateral` deposit before Take 16 — post it live in
  Take 17 if it isn't there yet).
- [ ] A real firmware binary file on disk for Take 15 (any file works; its
  SHA-256 becomes the demo hash, then approve it live).
- [ ] Small XLM balance for fees; small token amount for the deposit demo.
- [ ] Ranking: shoot takes in page order; the slash demo (16) needs the
  deposit (17) first if the owner has no collateral — check with
  **Check record** before rolling Take 16.

---

## INTRO — What SolNet is (~90s)

*Open on the landing page. Slow scroll through hero → contracts table.*

"SolNet is a community microgrid running on Stellar's Soroban smart
contracts — twelve of them, live on testnet. The problem it solves is trust.
In a neighborhood solar grid, who decides how much energy you used, what you
owe, whether your meter is honest, and where the community's money sits?
Usually that's one operator's spreadsheet — opaque, editable, and you just
have to believe it. SolNet replaces the spreadsheet with an on-chain ledger:
meter readings, billing, Watt-credit balances, firmware approvals, even
punishments for cheating meters are all recorded in smart contracts anyone
can read. This app is the window into that system — every number you see is
fetched live from the blockchain; there is no demo data anywhere.

One requirement before anything else: **connect a wallet.** We use Freighter.
Reads need it because the network simulates every query against your account,
and writes need it because every action is a transaction you personally sign.
If you're the grid operator, connect with the admin wallet — admin controls
across the app unlock only for the address each contract recognizes as its
authority. Let's walk the app."

*Click Connect, approve in Freighter, show the connected address.*

---

## Take 1 — Community dashboard (`/dashboard`)

**Say:** "This is the heartbeat. Your Watt-credit balance, read live from the
account ledger contract. Watt-credits are the grid's internal currency — one
credit, one unit of energy. As an admin, this page is your honesty check: if
a member disputes a bill, you read their balance here, on-chain, not from
anyone's records."

**Do:**
1. Show the connected wallet address.
2. Click **Read live contract**.
3. Hold on the `LIVE RESULT` balance value for 2 seconds.

---

## Take 2 — Add energy credit (`/payments`)

**Say:** "Here's where real value enters. You deposit actual tokens, and the
ledger mints Watt-credits one-to-one. Watch the warning on screen — real
tokens move. For the demo I'll use a small amount. As admin you don't approve
this; anyone can credit themselves. Your job is watching the backing stay
honest."

**Do:**
1. Enter a small amount (e.g. `10`).
2. **Review transaction** → show the exact args on camera.
3. **Confirm & sign**, approve in Freighter, hold on the tx hash.

---

## Take 3 — Withdraw energy credit (`/withdraw`)

**Say:** "The reverse trip: burn Watt-credits, release the backing tokens to
your account. This is the member's exit door, and it matters that it works
without asking permission — that's the proof the credits were really backed.
I'll burn a small amount and you'll see the tokens return."

**Do:** Same flow as Take 2 — amount → review → sign → hash. Mention the
*escrowed tokens return to you* explainer line on screen.

---

## Take 4 — Meters & billing (`/meters`)

**Say:** "Unsettled balances and reading counts per account, straight from the
micro-billing contract. This is the queue of energy consumed but not yet paid
for. Admins watch this page the way a shopkeeper watches the tab book — and
because it's on-chain, members can verify their own tab matches yours."

**Do:**
1. Leave the account field on the connected wallet (or paste a member address).
2. **Read live contract**, hold on unsettled balance + reading count.

---

## Take 5 — Delinquency assessment (`/emergency`)

**Say:** "What happens when someone doesn't pay? Not a phone call — a verdict.
This page assesses a live balance against the grace-period rules and returns
grace or disconnect, computed by the contract, not by a person. As admin you
read the verdict here before any disconnection is ever enforced. No
favoritism is technically possible."

**Do:**
1. Note the auto-filled ledger sequence; explain it in one sentence.
2. **Read live contract**, hold on the verdict.

---

## Take 6 — Network topology (`/topology`)

**Say:** "The physical grid, as the contracts see it: node count, neighbours,
reachability. When we seeded this network we registered real nodes on-chain,
and this page proves they're there. If a meter goes dark, this is where the
admin sees the hole in the map."

**Do:** **Read live contract** on `node_count`; scroll the neighbour data.
If node count reads 0 on a fresh deploy, say so honestly — then register a
node from the Admin page and re-read.

---

## Take 7 — Energy market (`/market`)

**Say:** "The grid's trading floor. Sellers list energy lots, buyers bid —
I'll place a bid against a live auction now, and you'll sign it in Freighter.
Admins don't set prices here; the auction does. Your role is listing
oversight and watching lots clear."

**Do:**
1. **Browse listings** → show an open auction.
2. **Use this ID**, enter a price, review, sign, hold on result.

---

## Take 8 — Live tariff (`/pricing`)

**Say:** "The current per-watt-hour price from the peak-load adjuster. When
demand spikes, the price moves automatically — no committee meeting. Short
page, important number: every bill on the grid traces back to this price."

**Do:** Single read; hold on the price. One take, no signing.

---

## Take 9 — Storage reserves (`/batteries`)

**Say:** "Battery reserves available for maintenance drawdowns. Community
batteries degrade, and replacing them costs money the grid must provably
hold. This page reads that reserve live. If it ever reads near zero, the
admin's job is to sound the alarm before the hardware fails."

**Do:** Read `available` (plus the auxiliary reserve read below); hold on
both numbers.

---

## Take 10 — Community capital (`/campaigns`)

**Say:** "The community fund itself — total capital backing it, read from the
fund contract. Members invested real value here, and this number is their
collective proof it still exists. As admin you never touch principal
casually; every movement is a signed, visible transaction."

**Do:** Single read of `total_capital`; hold on the value.

---

## Take 11 — Investor portfolio (`/portfolio`)

**Say:** "Your personal stake: share balance for the connected wallet. I'll
read mine now. Notice the discipline of this app — it shows *your* shares
because *your* wallet is connected. Connect a different wallet, see a
different truth. That's how members audit their own position."

**Do:** Read `shares_of`; emphasize the wallet→result linkage.

---

## Take 12 — Revenue & audit (`/audit`)

**Say:** "The audit trail. Raw Soroban events pulled from a selected contract
over recent ledgers — and note the page's own honesty label: these are raw
events, not a revenue statement. As admin, this is where you answer the hard
questions: show me every deposit, every slash, every settlement, with ledger
receipts. I'll run a query now and export the report."

**Do:**
1. Select a contract, run the event query.
2. Click export; show the downloaded JSON report briefly.

---

## Take 13 — Device registry (`/devices`)

**Say:** "Every meter has a cryptographic identity, and this is where they're
whitelisted. I'll decode my own wallet to its raw key to show the format —
but remember, a production meter key comes from the manufacturer's manifest,
not from software. Registration itself requires the admin key, which I hold.
No rogue meter can join the grid without your signature."

**Do:**
1. **Use connected wallet** → show the decoded raw key.
2. Enter a model name, review, sign the registration.

---

## Take 14 — Offline ingestion (`/offline-ingestion`)

**Say:** "Meters in the field often have no internet — they record readings
offline and submit them in signed batches. This page builds one: batch ID,
JSON readings, and a digest the contract itself computes so tampering is
detectable. I'll compute the digest, review the exact arguments, and sign.
Replay protection means each batch ID works exactly once."

**Do:**
1. **Randomize** the batch ID (say why out loud).
2. Keep/adjust the prefilled JSON entries.
3. **Compute digest** → **Review transaction** → sign → hash.
4. Optional second take: resubmit the same batch ID to show the replay
   rejection on camera.

---

## Take 15 — Firmware attestation (`/firmware`)

**Say:** "How do you know a meter runs approved software and not a hacked
build that lies about readings? Hash the firmware binary — I'll do it right
here in the browser, the file never leaves my machine — then check it against
the on-chain registry of community-approved builds. Approved or unapproved,
decided by the contract. And approving a new build is your admin power,
exercised on the Operator page."

**Do:**
1. **Use my wallet's key** (state out loud that the verdict judges the hash,
   not the key).
2. **Hash a firmware file** → field fills locally.
3. **Read live contract** → hold on the verdict.
4. Cut to `/admin` → **Approve firmware** with that hash → re-verify flips
   to `Approved` (strongest possible demo arc).

---

## Take 16 — Device incidents (`/incidents`)

**Say:** "The teeth. When a meter is proven to cheat — spoofed generation or
router bypass — you slash its posted collateral deposit. I'll check the
investigation record first: live collateral, prior slashes, appeals. Then
review and sign. Note the order the contract enforces: no deposit, no slash;
already slashed, can't slash twice; under appeal, hands off. The code
protects operators from *you*, too."

**Do:**
1. Enter owner + offence code → **Check record** → hold on all three rows.
2. If collateral is 0, say so, cut to Take 17, come back.
3. **Review transaction** → sign → hold on success.
4. Re-check the record to show the updated collateral on camera.

---

## Take 17 — Developer tools (`/developer-tools`)

**Say:** "The power-user bench. Any contract, any method, typed arguments,
simulate-before-you-sign. I used this exact screen to post the collateral
deposit behind that slash demo. Everything here still goes through
Freighter — there is no backdoor around signatures, including for admins."

**Do:**
1. Select `hardware_slash` → `deposit_collateral` → owner + amount.
2. **Simulate read** first (show it succeeds), then **Review transaction**,
   sign.
3. Mention the `offence` argument type exists for enum methods.

---

## Take 18 — Network settings (`/settings`)

**Say:** "Where the app points: RPC endpoint, network, and all twelve
contract IDs, stored only in this browser. If the grid ever migrates
contracts, this is the one place addresses change. Worth showing on camera so
viewers know these aren't hardcoded mysteries."

**Do:** Scroll the contract ID list slowly; do not change anything live
(or cut if you must edit).

---

## Take 19 — Operator admin (`/admin`)

**Say:** "And finally, your control room. Every section here — approve
firmware, slash, register devices, settle batches — unlocks only because my
connected wallet matches each contract's on-chain admin. Connect any other
wallet and these same buttons lock with a plain message: the chain would
reject the call. That lockout *is* the demo — authority in SolNet is
cryptographic, not cosmetic."

**Do:**
1. Scroll through the unlocked admin forms.
2. **The money shot:** disconnect/switch to a non-admin wallet on camera and
   show the forms locking. Switch back.

---

## OUTRO (~20s)

*Back on the landing page or dashboard.*

"That's SolNet: metering, billing, credit, capital, hardware trust, and
punishment — all on-chain, all readable, all signed. Connect a wallet and
verify any claim I just made; that's the entire point."
