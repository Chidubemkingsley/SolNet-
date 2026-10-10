import { useEffect, useMemo, useState, createContext, useContext, type ReactNode } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ErrorBoundary } from '@/components/error-boundary';
import { Toaster } from '@/components/ui/toaster';
import { TooltipProvider } from '@/components/ui/tooltip';
import NotFound from '@/pages/not-found';
import { Route, Switch, useLocation, Router as WouterRouter, Link } from 'wouter';
import {
  Activity, ArrowDownToLine, BatteryCharging, Blocks, BookOpenCheck, Box, Cable,
  ChevronRight, CircleDollarSign, Cpu, Database, FileClock, FileText, Gauge,
  HardDrive, Landmark, Menu, Network, Radio, RefreshCw, ShieldAlert, SlidersHorizontal,
  Sparkles, Vote, Wallet, X, Zap, Settings as SettingsIcon, Wrench, ChartNoAxesCombined,
} from 'lucide-react';
import {
  assertWalletNetwork, checkRpcStatus, connectStellarWallet, isValidContractAddress, NETWORKS,
  getNetworkPassphrase, readConnectedWallet, shortenAddress, simulateContractRead, submitContractTransaction,
  fetchContractEvents, toDisplayJson, verifyContractDeployment, type ContractArgument, type NetworkStatus,
  type SorobanArgumentType,
} from '@/lib/helper';
import { CONTRACTS, CONTRACT_BY_KEY, defaultArguments, type ContractKey, type ContractMethod } from '@/lib/contracts';
import { loadSettings, saveSettings, type SolNetSettings } from '@/lib/config';

const client = new QueryClient();
type Outcome = { kind: 'read' | 'tx' | 'error'; text: string; hash?: string; status?: string };
interface AppState {
  settings: SolNetSettings;
  wallet: string | null;
  setWallet: (value: string | null) => void;
  network: NetworkStatus | null;
  networkError: string;
  refreshNetwork: () => Promise<void>;
}
const StateContext = createContext<AppState | null>(null);
const useAppState = () => {
  const value = useContext(StateContext);
  if (!value) throw new Error('SolNet application state is unavailable.');
  return value;
};

type Item = { path: string; title: string; summary: string; section: string; icon: typeof Gauge; contract?: ContractKey; method?: string; kind?: 'read' | 'write' | 'none' };
const PAGES: Item[] = [
  { path:'/dashboard', title:'Community dashboard', summary:'Your account and neighborhood energy status, read directly from the configured network.', section:'Community', icon:Gauge, contract:'account_ledger', method:'get_balance', kind:'read' },
  { path:'/payments', title:'Add energy credit', summary:'Convert a stablecoin deposit into community energy credit through the deployed account ledger.', section:'Community', icon:ArrowDownToLine, contract:'account_ledger', method:'deposit_stablecoin', kind:'write' },
  { path:'/meters', title:'Meters & billing', summary:'Meter telemetry and billing history are shown only when the configured contract returns them.', section:'Community', icon:Activity, contract:'micro_billing', method:'get_meter_logs', kind:'read' },
  { path:'/emergency', title:'Emergency support', summary:'Review disconnection status or request an emergency extension for a registered meter.', section:'Community', icon:ShieldAlert, contract:'delinquency_disconnector', method:'check_disconnection_status', kind:'read' },
  { path:'/topology', title:'Network topology', summary:'Neighborhood nodes and physical routing reported by the live grid contract.', section:'Operations', icon:Network, contract:'grid_topology', method:'get_network_nodes', kind:'read' },
  { path:'/market', title:'Energy market', summary:'Submit a bid or inspect the peer-to-peer auction interface. Unknown ask arguments are not inferred.', section:'Operations', icon:ChartNoAxesCombined, contract:'yield_auction', method:'place_energy_bid', kind:'write' },
  { path:'/pricing', title:'Live tariff', summary:'Current tariff values are calculated by the configured peak-load contract.', section:'Operations', icon:CircleDollarSign, contract:'peak_load_adjuster', method:'calculate_current_tariff', kind:'read' },
  { path:'/batteries', title:'Storage reserves', summary:'Community pool reserves and battery health from on-chain contract reads.', section:'Operations', icon:BatteryCharging, contract:'battery_pool', method:'get_pool_reserves', kind:'read' },
  { path:'/campaigns', title:'Community campaigns', summary:'Funding campaigns currently returned by the community capital contract.', section:'Capital & governance', icon:Sparkles, contract:'community_capital', method:'get_active_campaigns', kind:'read' },
  { path:'/portfolio', title:'Investor portfolio', summary:'Read investor yield share for the currently connected wallet.', section:'Capital & governance', icon:Landmark, contract:'community_capital', method:'get_investor_yield_share', kind:'read' },
  { path:'/proposals', title:'Capital proposals', summary:'Submit capital expenditure proposals or vote where the deployed ABI is known.', section:'Capital & governance', icon:FileText, contract:'community_capital', method:'submit_capex_proposal', kind:'write' },
  { path:'/audit', title:'Revenue & audit', summary:'Review raw Soroban events from a selected deployed contract. Revenue semantics require the deployed contract ABI.', section:'Capital & governance', icon:BookOpenCheck, kind:'none' },
  { path:'/devices', title:'Device registry', summary:'Inspect or register authorized hardware identities through the configured IoT registrar.', section:'Field systems', icon:Cpu, contract:'iot_registrar', method:'register_new_device', kind:'write' },
  { path:'/offline-ingestion', title:'Offline ingestion', summary:'Submit signed field telemetry using the registered contract method; its ABI arguments are unspecified.', section:'Field systems', icon:Radio, contract:'cryptographic_oracle', method:'submit_offline_telemetry_batch', kind:'write' },
  { path:'/firmware', title:'Firmware attestation', summary:'Verify a device firmware hash against the configured firmware contract.', section:'Field systems', icon:HardDrive, contract:'firmware_verifier', method:'verify_device_firmware', kind:'read' },
  { path:'/incidents', title:'Device incidents', summary:'Investigate an incident and submit evidence only with the deployed contract and explicitly entered hashes.', section:'Field systems', icon:ShieldAlert, contract:'hardware_slash', method:'initiate_slash_sequence', kind:'write' },
  { path:'/parameters', title:'Protocol parameters', summary:'Administrative parameter methods were not specified. Use typed RPC tools only when the deployed ABI is known.', section:'Protocol', icon:SlidersHorizontal, kind:'none' },
  { path:'/governance', title:'Governance', summary:'Read live proposals and cast a known-form vote. Proposal submission ABI remains unspecified.', section:'Protocol', icon:Vote, contract:'community_capital', method:'cast_proposal_vote', kind:'write' },
  { path:'/treasury', title:'Treasury distribution', summary:'Treasury distribution methods were not specified. Inspect live reserves or use typed RPC tools; no payout method is guessed.', section:'Protocol', icon:CircleDollarSign, kind:'none' },
  { path:'/developer-tools', title:'Developer tools', summary:'Run read simulations and carefully review typed contract invocations before Freighter signing.', section:'Protocol', icon:Blocks, kind:'none' },
  { path:'/settings', title:'Network settings', summary:'Configure RPC, network, and real Soroban contract IDs. Changes are stored in this browser.', section:'System', icon:SettingsIcon, kind:'none' },
];

const TYPE_OPTIONS: SorobanArgumentType[] = ['address','bool','string','symbol','bytes','bytes32','i32','u32','i64','u64','i128','u128','i256','u256','json'];
function titleFor(path: string) { return PAGES.find(item => item.path === path) ?? PAGES[0]; }

function App() {
  const [settings, setSettings] = useState<SolNetSettings>(() => loadSettings());
  const [wallet, setWallet] = useState<string | null>(null);
  const [network, setNetwork] = useState<NetworkStatus | null>(null);
  const [networkError, setNetworkError] = useState('');
  const [walletError, setWalletError] = useState('');
  const [menuOpen, setMenuOpen] = useState(false);
  const config = useMemo(() => ({ network: settings.network, rpcUrl: settings.rpcUrl, customNetworkPassphrase: settings.customNetworkPassphrase }), [settings]);
  const refreshNetwork = async () => {
    setNetworkError('');
    try { setNetwork(await checkRpcStatus(config)); }
    catch (error) { setNetwork(null); setNetworkError(error instanceof Error ? error.message : 'RPC health check failed.'); }
  };
  useEffect(() => {
    void refreshNetwork();
    const timer = window.setInterval(() => void refreshNetwork(), 15_000);
    return () => window.clearInterval(timer);
  }, [settings.network, settings.rpcUrl, settings.customNetworkPassphrase]);
  useEffect(() => {
    let active = true;
    readConnectedWallet().then(address => { if (active) setWallet(address); }).catch(error => { if (active) setWalletError(error instanceof Error ? error.message : 'Could not read wallet connection.'); });
    return () => { active = false; };
  }, []);
  const updateSettings = (next: SolNetSettings) => { setSettings(next); saveSettings(next); };
  const context: AppState = { settings, wallet, setWallet, network, networkError, refreshNetwork };
  return <QueryClientProvider client={client}><TooltipProvider><WouterRouter base={import.meta.env.BASE_URL.replace(/\/$/, '')}>
    <StateContext.Provider value={context}>
      <RoutedErrorBoundary>
        <Switch>
          <Route path="/"><RedirectDashboard /></Route>
          {PAGES.map(page => <Route key={page.path} path={page.path}><Shell page={page} menuOpen={menuOpen} setMenuOpen={setMenuOpen} walletError={walletError} setWalletError={setWalletError} updateSettings={updateSettings} /></Route>)}
          <Route component={NotFound} />
        </Switch>
      </RoutedErrorBoundary>
    </StateContext.Provider>
  </WouterRouter><Toaster /></TooltipProvider></QueryClientProvider>;
}

function RedirectDashboard() {
  const [, navigate] = useLocation();
  useEffect(() => { navigate('/dashboard', { replace: true }); }, [navigate]);
  return null;
}

function RoutedErrorBoundary({ children }: { children: ReactNode }) {
  const [location] = useLocation();
  return <ErrorBoundary resetKey={location}>{children}</ErrorBoundary>;
}

function Shell({ page, menuOpen, setMenuOpen, walletError, setWalletError, updateSettings }: {
  page: Item; menuOpen: boolean; setMenuOpen: (open: boolean) => void; walletError: string;
  setWalletError: (error: string) => void; updateSettings: (settings: SolNetSettings) => void;
}) {
  const { settings, wallet, setWallet, network, networkError, refreshNetwork } = useAppState();
  const [path] = useLocation();
  const [connecting, setConnecting] = useState(false);
  const connect = async () => {
    setWalletError(''); setConnecting(true);
    try { const address = await connectStellarWallet(settings); setWallet(address); }
    catch (error) { setWalletError(error instanceof Error ? error.message : 'Wallet connection failed.'); }
    finally { setConnecting(false); }
  };
  return <div className="app-shell">
    {menuOpen && <button className="mobile-scrim" aria-label="Close navigation" onClick={() => setMenuOpen(false)} data-testid="button-close-navigation" />}
    <aside className={`sidebar ${menuOpen ? 'open' : ''}`}>
      <Link href="/dashboard" className="brand" onClick={() => setMenuOpen(false)} data-testid="link-solnet-home">
        <span className="brand-mark"><Cable size={20} strokeWidth={2.2} /></span><span><span className="brand-name">SolNet</span><span className="brand-sub">Microgrid console</span></span>
      </Link>
      <nav aria-label="Primary navigation">
        {['Community','Operations','Capital & governance','Field systems','Protocol','System'].map(section => <div key={section}>
          <div className="nav-section">{section}</div><div className="nav-list">
            {PAGES.filter(item => item.section === section).map(item => {
              const Icon = item.icon;
              return <Link href={item.path} key={item.path} onClick={() => setMenuOpen(false)} className={`nav-link ${path === item.path ? 'active' : ''}`} data-testid={`link-nav-${item.path.slice(1)}`}>
                <Icon aria-hidden="true" /><span>{item.title}</span>
              </Link>;
            })}
          </div>
        </div>)}
      </nav>
      <div className="sidebar-bottom"><div className="net-mini"><span className="pulse-dot" style={network ? { background:'#70b79a' } : {}} /><span>{network ? `${network.network} · ledger ${network.latestLedger}` : 'Network status unavailable'}</span></div></div>
    </aside>
    <main className="shell-main">
      <header className="topbar">
        <div className="row" style={{justifyContent:'flex-start', gap:10}}><button className="mobile-menu" onClick={() => setMenuOpen(true)} aria-label="Open navigation" data-testid="button-open-navigation"><Menu size={19} /></button><span className="crumb">SolNet / <b>{page.title}</b></span></div>
        <div className="top-actions">
          <div className="network-pill" data-testid="status-network-health"><span className="pulse-dot" style={network ? {background:'#3a9875'} : {background:'#bd8b45'}} />{network ? `${settings.network.toUpperCase()} · ${network.latencyMs}ms` : 'RPC UNAVAILABLE'}</div>
          {wallet ? <button className="wallet-btn secondary" onClick={() => { setWallet(null); setWalletError(''); }} data-testid="button-wallet-disconnect"><Wallet size={14} />{shortenAddress(wallet)}<X size={13} /></button> :
            <button className="wallet-btn" disabled={connecting} onClick={connect} data-testid="button-wallet-connect"><Wallet size={14} />{connecting ? 'Connecting…' : 'Connect wallet'}</button>}
        </div>
      </header>
      <div className="content animate-in" key={path}>
        {walletError && <div className="notice error" data-testid="status-wallet-error" style={{marginBottom:14}}>{walletError}</div>}
        {networkError && <div className="notice neutral row" data-testid="status-rpc-error" style={{marginBottom:14}}><span>{networkError}</span><button className="button ghost" onClick={() => void refreshNetwork()} data-testid="button-rpc-retry"><RefreshCw size={13} />Retry RPC</button></div>}
        {page.path === '/settings' ? <SettingsView updateSettings={updateSettings} /> :
          page.path === '/developer-tools' ? <DeveloperView /> : page.path === '/audit' ? <AuditView /> :
            page.path === '/treasury' || page.path === '/parameters' ? <UnspecifiedInterfaceView page={page} /> : <OperationalView page={page} />}
      </div>
    </main>
  </div>;
}

function PageHeading({ page, actions }: { page: Item; actions?: ReactNode }) {
  const Icon = page.icon;
  return <div className="page-heading"><div><div className="eyebrow"><Icon size={12} style={{verticalAlign:'-2px',marginRight:6}} />{page.section} / Live console</div><h1>{page.title}</h1><p className="page-desc">{page.summary}</p></div>{actions && <div className="heading-actions">{actions}</div>}</div>;
}

function EmptyState({ title, children, testid = 'empty-live-data' }: { title: string; children: ReactNode; testid?: string }) {
  return <div className="empty-state" data-testid={testid}><strong>{title}</strong>{children}</div>;
}
function StatusTag({ children, tone = '' }: { children: ReactNode; tone?: string }) {
  return <span className={`status ${tone}`} data-testid="status-data-state"><span className="status-dot" />{children}</span>;
}

function OperationalView({ page }: { page: Item }) {
  const { settings, wallet } = useAppState();
  const [meterId, setMeterId] = useState('');
  const [amount, setAmount] = useState('');
  const [deviceId, setDeviceId] = useState('');
  const [hash, setHash] = useState('');
  const [account, setAccount] = useState('');
  const [proposalId, setProposalId] = useState('');
  const [support, setSupport] = useState(true);
  const [readResult, setReadResult] = useState<Outcome | null>(null);
  const [busy, setBusy] = useState(false);
  const [review, setReview] = useState(false);
  const contract = page.contract ? CONTRACT_BY_KEY[page.contract] : undefined;
  const method = contract?.methods.find(entry => entry.name === page.method);
  const contractId = page.contract ? settings.contracts[page.contract] : '';
  const args = useMemo(() => {
    if (!method) return [];
    if (page.path === '/dashboard') return [{name:'user',type:'address' as const,value:account || wallet || ''}];
    if (page.path === '/portfolio') return [{name:'investor',type:'address' as const,value:account || wallet || ''}];
    if (page.path === '/payments') return [{name:'user',type:'address' as const,value:wallet || ''},{name:'amount',type:'u128' as const,value:amount}];
    if (page.path === '/meters' || page.path === '/emergency') return [{name:'meter_id',type:'bytes32' as const,value:meterId}];
    if (page.path === '/market') return [{name:'bidder',type:'address' as const,value:account || wallet || ''},{name:'watt_hours',type:'u64' as const,value:amount},{name:'max_price',type:'u128' as const,value:hash}];
    if (page.path === '/firmware') return [{name:'device_id',type:'bytes32' as const,value:deviceId},{name:'active_hash',type:'bytes32' as const,value:hash}];
    if (page.path === '/governance') return [{name:'proposal_id',type:'u32' as const,value:proposalId},{name:'support',type:'bool' as const,value:support}];
    if (page.path === '/devices') return [{name:'device_id',type:'bytes32' as const,value:deviceId},{name:'hardware_pubkey',type:'address' as const,value:account || wallet || ''}];
    if (page.path === '/incidents') return [{name:'malicious_device',type:'bytes32' as const,value:deviceId},{name:'evidence_hash',type:'bytes32' as const,value:hash}];
    return defaultArguments(method);
  }, [method, page.path, account, wallet, amount, meterId, hash, deviceId, proposalId, support]);
  const incomplete = args.some(arg => arg.value === '' || (typeof arg.value === 'string' && !arg.value.trim()));
  const canRead = !!method && method.access === 'read' && !!wallet && isValidContractAddress(contractId) && !incomplete;
  const canWrite = !!method && method.access === 'write' && !!wallet && isValidContractAddress(contractId) && !incomplete;
  const runRead = async () => {
    if (!page.contract || !method) return;
    setBusy(true); setReadResult(null);
    try {
      await verifyContractDeployment(contractId, settings);
      const value = await simulateContractRead({ config: settings, sourceAddress: wallet!, contractId, method: method.name, args });
      setReadResult({kind:'read',text:toDisplayJson(value)});
    } catch (error) { setReadResult({kind:'error',text:error instanceof Error ? error.message : 'Contract read failed.'}); }
    finally { setBusy(false); }
  };
  const submit = async () => {
    if (!page.contract || !method) return;
    setBusy(true); setReview(false); setReadResult(null);
    try {
      await verifyContractDeployment(contractId, settings);
      const result = await submitContractTransaction({config:settings,sourceAddress:wallet!,contractId,method:method.name,args});
      setReadResult({kind:'tx',text:result.status === 'SUCCESS' ? 'RPC confirmed this transaction on-chain.' : 'RPC accepted this transaction and it remains pending.',hash:result.hash,status:result.status});
    } catch(error) { setReadResult({kind:'error',text:error instanceof Error ? error.message : 'Transaction was not confirmed by RPC.'}); }
    finally { setBusy(false); }
  };

  return <div>
    <PageHeading page={page} actions={<StatusTag tone={contractId && isValidContractAddress(contractId) ? 'ok' : 'warn'}>{contractId && isValidContractAddress(contractId) ? 'CONTRACT ID SET' : 'UNCONFIGURED'}</StatusTag>} />
    {page.path === '/dashboard' ? <DashboardOverview /> : null}
    <div className="grid cols-2" style={{alignItems:'start'}}>
      <section className="card">
        <div className="card-header"><div><h2 className="card-title">On-chain interface</h2><div className="card-subtitle">{contract?.label ?? 'Configured contract'} · {method?.name ?? 'No method selected'}</div></div><span className="metric-icon"><Database /></span></div>
        <div className="field"><label>Target contract</label><div className="input mono" data-testid="text-target-contract">{contractId || 'No contract ID configured'}</div><div className="field-help">{contractId && !isValidContractAddress(contractId) ? 'This value is not a valid Soroban contract address.' : 'Target and method are read from the supplied contract registry.'} <Link href="/settings" className="inline-code" data-testid="link-configure-contract">Configure network & IDs</Link></div></div>
        <div className="field"><label>Method</label><div className="input mono" data-testid="text-target-method">{method?.name ?? 'Not defined in contract registry'}</div></div>
        {contract && <div className="notice neutral" style={{marginBottom:14}}>{contract.description} Interfaces are not independently ABI-verified; only the supplied registry is used.</div>}
        {!wallet && <div className="notice" style={{marginBottom:13}}>Connect Freighter to read contract state. Simulation requires a source account and a matching network.</div>}
      {page.path === '/dashboard' || page.path === '/portfolio' ? <AddressField label={page.path === '/portfolio' ? 'Investor address' : 'Account address'} value={account} onChange={setAccount} fallback={wallet} testid="input-account-address" /> : null}
        {(page.path === '/meters' || page.path === '/emergency') && <TextField label="Meter ID · 32-byte hex" value={meterId} onChange={setMeterId} placeholder="Enter registered meter ID" testid="input-meter-id" />}
        {(page.path === '/payments' || page.path === '/market') && <TextField label={page.path === '/payments' ? 'Deposit amount · smallest token units' : 'Energy amount · watt-hours'} value={amount} onChange={setAmount} placeholder="Enter integer amount" testid="input-amount" />}
        {page.path === '/market' && <TextField label="Maximum price · smallest token units" value={hash} onChange={setHash} placeholder="Enter integer price" testid="input-max-price" />}
        {(page.path === '/firmware' || page.path === '/devices' || page.path === '/incidents') && <TextField label="Device ID · 32-byte hex" value={deviceId} onChange={setDeviceId} placeholder="Enter device identifier" testid="input-device-id" />}
        {(page.path === '/firmware' || page.path === '/incidents') && <TextField label={page.path === '/firmware' ? 'Active firmware hash · 32-byte hex' : 'Evidence hash · 32-byte hex'} value={hash} onChange={setHash} placeholder="Enter exact hash" testid="input-evidence-hash" />}
        {page.path === '/devices' && <AddressField label="Hardware public key address" value={account} onChange={setAccount} fallback={wallet} testid="input-hardware-address" />}
        {page.path === '/governance' && <><TextField label="Proposal ID" value={proposalId} onChange={setProposalId} placeholder="Enter on-chain proposal number" testid="input-proposal-id" /><label className="row" style={{justifyContent:'flex-start',fontSize:10,gap:8,marginBottom:13}}><input type="checkbox" checked={support} onChange={e=>setSupport(e.target.checked)} data-testid="input-vote-support" /> Support proposal</label></>}
        {page.kind === 'read' && <button className="button" disabled={!canRead || busy} onClick={() => void runRead()} data-testid="button-read-contract"><RefreshCw size={13} />{busy ? 'Reading…' : 'Read live contract'}</button>}
        {page.kind === 'write' && <div>
          <div className="notice" style={{marginBottom:12}}>A transaction is not sent until you review the exact arguments and confirm. Freighter will request the final signature.</div>
          {!review ? <button className="button" disabled={!canWrite || busy} onClick={() => setReview(true)} data-testid="button-review-transaction">Review transaction <ChevronRight size={14} /></button> :
            <div className="notice neutral" data-testid="panel-transaction-review">
              <b>Confirm this invocation</b><div className="divider" />
              <div className="field-help">Contract · <span className="mono">{contractId}</span></div><div className="field-help">Method · <span className="mono">{method?.name}</span></div>
              <pre className="result-box" style={{maxHeight:150,margin:'9px 0 12px'}}>{toDisplayJson(args)}</pre>
              <div className="row" style={{justifyContent:'flex-start'}}><button className="button" disabled={busy} onClick={() => void submit()} data-testid="button-confirm-transaction">{busy ? 'Awaiting RPC…' : 'Confirm & sign with Freighter'}</button><button className="button ghost" onClick={() => setReview(false)} data-testid="button-cancel-review">Cancel</button></div>
            </div>}
        </div>}
        {method?.access === 'unknown' && <div className="notice" style={{marginTop:12}}>The supplied registry marks this ABI as unknown. No arguments are inferred here. <Link href="/developer-tools" className="inline-code" data-testid="link-unknown-abi-tools">Define typed arguments in Developer tools</Link>.</div>}
      </section>
      <section className="card">
        <div className="card-header"><div><h2 className="card-title">Live result</h2><div className="card-subtitle">Only values returned by RPC appear here.</div></div><StatusTag tone={readResult?.kind === 'read' || readResult?.kind === 'tx' ? 'ok' : readResult?.kind === 'error' ? 'error' : ''}>{readResult ? readResult.kind === 'error' ? 'RPC ERROR' : readResult.kind === 'tx' ? readResult.status ?? 'SUBMITTED' : 'LIVE RESULT' : 'NO READ YET'}</StatusTag></div>
        {busy ? <div className="skeleton-stack" data-testid="loading-contract-result"><div className="skeleton-line"/><div className="skeleton-line"/><div className="skeleton-line"/></div> : readResult ? readResult.kind === 'error' ? <div className="notice error" data-testid="status-operation-error">{readResult.text}</div> : readResult.kind === 'tx' ? <div><div className="notice neutral" data-testid="status-transaction-result">{readResult.text}</div><div className="section-kicker">Transaction hash</div><div className="result-box" data-testid="text-transaction-hash">{readResult.hash}</div><div className="section-kicker">RPC status</div><span className={`status ${readResult.status === 'SUCCESS' ? 'ok' : 'warn'}`} data-testid="status-transaction-state">{readResult.status}</span></div> : <pre className="result-box" data-testid="data-contract-read">{readResult.text}</pre> :
          <EmptyState title="No live result available">Read a configured, deployed contract to populate this panel. No sample balances, events, or telemetry are displayed.</EmptyState>}
        {page.path === '/meters' && <div className="section-kicker">Meter history</div>}
        {page.path === '/audit' && <div className="section-kicker">Accounting proof</div>}
        {page.path === '/batteries' && <div className="section-kicker">Reserve and health data</div>}
        {page.path === '/topology' && <div className="section-kicker">Network node data</div>}
      </section>
    </div>
    {page.path === '/dashboard' && <DashboardSecondary />}
    {page.path === '/emergency' && <EmergencyExtension meterId={meterId} />}
    {page.path === '/batteries' && <AuxiliaryRead contractKey="battery_pool" methodName="get_battery_health_index" title="Battery health index" />}
    {page.path === '/payments' && <div className="section-kicker">Deposit flow · stablecoin to energy credit</div>}
    {page.path === '/campaigns' && <div className="section-kicker">Funding opportunities</div>}
    {page.path === '/incidents' && <div className="section-kicker">Investigation record</div>}
  </div>;
}

function AuditView() {
  const { settings, network } = useAppState();
  const [contractKey, setContractKey] = useState<ContractKey>('account_ledger');
  const [startLedger, setStartLedger] = useState('');
  const [result, setResult] = useState<(Awaited<ReturnType<typeof fetchContractEvents>> & { contractId: string; startLedger: number }) | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const contract = CONTRACT_BY_KEY[contractKey];
  const contractId = settings.contracts[contractKey];
  const query = async () => {
    if (!network) { setError('Wait for the selected RPC network check to complete.'); return; }
    const start = startLedger.trim() ? Number(startLedger) : Math.max(1, network.latestLedger - 999);
    setBusy(true); setError(''); setResult(null);
    try {
      const response = await fetchContractEvents(settings, contractId, start, 100);
      setResult({ ...response, contractId, startLedger: start });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'RPC event query failed.');
    } finally { setBusy(false); }
  };
  const exportReport = () => {
    if (!result) return;
    const report = {
      source: 'Stellar Soroban RPC getEvents',
      generatedAt: new Date().toISOString(),
      network: settings.network,
      rpcUrl: settings.rpcUrl,
      contractId: result.contractId,
      startLedger: result.startLedger,
      oldestAvailableLedger: result.oldestLedger,
      latestLedger: result.latestLedger,
      eventCount: result.events.length,
      events: result.events,
      note: 'Raw RPC event records; not a cryptographic inclusion proof or an ABI-interpreted revenue statement.',
    };
    const url = URL.createObjectURL(new Blob([toDisplayJson(report)], { type: 'application/json' }));
    const link = document.createElement('a');
    link.href = url; link.download = `solnet-events-${result.startLedger}-${result.latestLedger}.json`;
    link.click(); URL.revokeObjectURL(url);
  };
  return <div>
    <PageHeading page={titleFor('/audit')} />
    <div className="grid cols-2" style={{alignItems:'start'}}>
      <section className="card">
        <div className="card-header"><div><h2 className="card-title">Soroban event evidence</h2><div className="card-subtitle">Public, raw contract events returned by the selected RPC.</div></div><span className="metric-icon"><BookOpenCheck /></span></div>
        <div className="notice neutral" style={{marginBottom:13}}>The project brief does not define revenue-event schemas. This view exports raw event data without inventing accounting meaning or claiming a cryptographic proof.</div>
        <div className="field"><label>Deployed contract</label><select className="select" value={contractKey} onChange={event=>{setContractKey(event.target.value as ContractKey);setResult(null);setError('');}} data-testid="select-audit-contract">{CONTRACTS.map(item=><option key={item.key} value={item.key}>{item.label}</option>)}</select></div>
        <div className="field"><label>Contract ID</label><div className="input mono" data-testid="text-audit-contract">{contractId || 'No contract ID configured'}</div><div className="field-help"><Link href="/settings" className="inline-code">Configure real deployed IDs</Link></div></div>
        <div className="field"><label>Start ledger (optional)</label><input className="input mono" type="number" min="1" step="1" value={startLedger} onChange={event=>setStartLedger(event.target.value)} placeholder={network ? String(Math.max(1,network.latestLedger-999)) : 'Waiting for RPC health'} data-testid="input-audit-start-ledger" /><div className="field-help">If blank, queries from the latest ledger minus 999. The RPC retention window may limit older history.</div></div>
        <div className="row" style={{justifyContent:'flex-start',flexWrap:'wrap'}}><button className="button" disabled={!network||!isValidContractAddress(contractId)||busy} onClick={()=>void query()} data-testid="button-query-audit-events"><RefreshCw size={13}/>{busy?'Querying RPC…':'Fetch live events'}</button>{result&&<button className="button secondary" onClick={exportReport} data-testid="button-export-audit-events"><ArrowDownToLine size={13}/>Export JSON</button>}</div>
        {error&&<div className="notice error" style={{marginTop:12}} data-testid="status-audit-error">{error}</div>}
      </section>
      <section className="card">
        <div className="card-header"><div><h2 className="card-title">Event query result</h2><div className="card-subtitle">No records are shown until returned by Soroban RPC.</div></div><StatusTag tone={result?'ok':error?'error':''}>{result?`${result.events.length} EVENTS`:error?'RPC ERROR':'NOT QUERIED'}</StatusTag></div>
        {result?<><div className="section-kicker" data-testid="status-audit-range">Ledgers {result.startLedger}–{result.latestLedger} · retained from {result.oldestLedger}</div>{result.events.length?<pre className="result-box" data-testid="data-audit-events">{toDisplayJson(result.events)}</pre>:<EmptyState title="No matching events in this ledger range">The selected RPC returned no matching events for this contract.</EmptyState>}</>:
          <EmptyState title="No event query yet">Choose a configured deployed contract to query its raw Soroban events.</EmptyState>}
      </section>
    </div>
  </div>;
}

function UnspecifiedInterfaceView({ page }: { page: Item }) {
  const related = page.path === '/treasury'
    ? { path: '/batteries', label: 'Read configured storage reserves', detail: 'The battery-pool reserve method is defined; treasury distribution signatures were not.' }
    : { path: '/pricing', label: 'Read the live tariff', detail: 'The tariff calculation is defined; administrative parameter methods were not.' };
  return <div>
    <PageHeading page={page} />
    <section className="card">
      <div className="card-header"><div><h2 className="card-title">No verified method supplied</h2><div className="card-subtitle">The interface is not guessed from a method name.</div></div><span className="metric-icon"><Database /></span></div>
      <div className="notice neutral" style={{marginBottom:13}}>{related.detail} Add the deployed contract ABI or verify the method signature before making state changes.</div>
      <div className="row" style={{justifyContent:'flex-start',flexWrap:'wrap'}}>
        <Link className="button secondary" href={related.path}>{related.label}</Link>
        <Link className="button" href="/developer-tools"><Wrench size={13}/>Open typed developer tools</Link>
      </div>
    </section>
  </div>;
}

function EmergencyExtension({ meterId }: { meterId:string }) {
  const {settings,wallet}=useAppState();
  const [review,setReview]=useState(false);
  const [busy,setBusy]=useState(false);
  const [outcome,setOutcome]=useState<Outcome|null>(null);
  const contractId=settings.contracts.delinquency_disconnector;
  const args:ContractArgument[]=[{name:'meter_id',type:'bytes32',value:meterId}];
  const ready=!!wallet&&isValidContractAddress(contractId)&&meterId.trim().length>0;
  const send=async()=>{
    setBusy(true);setReview(false);setOutcome(null);
    try{
      await verifyContractDeployment(contractId,settings);
      const response=await submitContractTransaction({config:settings,sourceAddress:wallet!,contractId,method:'request_emergency_extension',args});
      setOutcome({kind:'tx',text:response.status==='SUCCESS'?'RPC confirmed the emergency extension request.':'RPC accepted the request; it is still pending.',hash:response.hash,status:response.status});
    }catch(error){setOutcome({kind:'error',text:error instanceof Error?error.message:'RPC could not submit the extension request.'});}
    finally{setBusy(false);}
  };
  return <section className="card" style={{marginTop:15}}>
    <div className="card-header"><div><h2 className="card-title">Request emergency extension</h2><div className="card-subtitle">Known registry signature · delinquency_disconnector.request_emergency_extension(meter_id: bytes32)</div></div><span className="metric-icon"><ShieldAlert/></span></div>
    <p className="field-help" style={{marginBottom:12}}>Uses the meter ID entered above. The read status check and the extension request are separate on-chain operations.</p>
    {!review?<button className="button" disabled={!ready||busy} onClick={()=>setReview(true)} data-testid="button-review-emergency-extension">Review extension request</button>:<div className="notice neutral" data-testid="panel-emergency-review"><b>Confirm extension request</b><div className="field-help" style={{marginTop:6}}>Target · <span className="mono">{contractId}</span></div><div className="field-help">Method · <span className="mono">request_emergency_extension</span></div><pre className="result-box" style={{margin:'10px 0'}}>{toDisplayJson(args)}</pre><div className="row" style={{justifyContent:'flex-start'}}><button className="button" disabled={busy} onClick={()=>void send()} data-testid="button-confirm-emergency-extension">{busy?'Awaiting RPC…':'Confirm & sign with Freighter'}</button><button className="button ghost" onClick={()=>setReview(false)} data-testid="button-cancel-emergency-extension">Cancel</button></div></div>}
    {outcome&&<div style={{marginTop:12}}>{outcome.kind==='error'?<div className="notice error" data-testid="status-emergency-error">{outcome.text}</div>:<><div className="notice neutral" data-testid="status-emergency-result">{outcome.text}</div><div className="section-kicker">Transaction hash · {outcome.status}</div><div className="result-box" data-testid="text-emergency-hash">{outcome.hash}</div></>}</div>}
    {!contractId&&<div className="field-help" style={{marginTop:10}}>The disconnection contract is not configured. Set its real ID in Network settings.</div>}
  </section>;
}

function AuxiliaryRead({contractKey,methodName,title}:{contractKey:ContractKey;methodName:string;title:string}){
  const {settings,wallet}=useAppState();
  const [value,setValue]=useState<Outcome|null>(null);
  const [busy,setBusy]=useState(false);
  const contract=CONTRACT_BY_KEY[contractKey];
  const method=contract.methods.find(item=>item.name===methodName);
  const contractId=settings.contracts[contractKey];
  const read=async()=>{
    if(!wallet||!method)return;
    setBusy(true);setValue(null);
    try{
      await verifyContractDeployment(contractId,settings);
      const response=await simulateContractRead({config:settings,sourceAddress:wallet,contractId,method:method.name,args:[]});
      setValue({kind:'read',text:toDisplayJson(response)});
    }catch(error){setValue({kind:'error',text:error instanceof Error?error.message:'RPC read failed.'});}
    finally{setBusy(false);}
  };
  return <section className="card" style={{marginTop:15}}>
    <div className="card-header"><div><h2 className="card-title">{title}</h2><div className="card-subtitle mono">{contractKey}.{methodName}()</div></div><StatusTag tone={value?.kind==='read'?'ok':value?.kind==='error'?'error':''}>{value?value.kind==='error'?'ERROR':'LIVE DATA':'NO READ'}</StatusTag></div>
    <button className="button secondary" disabled={!wallet||!isValidContractAddress(contractId)||busy} onClick={()=>void read()} data-testid="button-read-battery-health"><RefreshCw size={13}/>{busy?'Reading…':'Read battery health index'}</button>
    {!wallet&&<div className="field-help" style={{marginTop:8}}>Connect a wallet to use as the simulation source.</div>}
    {!contractId&&<div className="field-help" style={{marginTop:8}}>Battery pool contract ID is unconfigured.</div>}
    {value&&<div style={{marginTop:12}}>{value.kind==='error'?<div className="notice error" data-testid="status-battery-error">{value.text}</div>:<pre className="result-box" data-testid="data-battery-health">{value.text}</pre>}</div>}
  </section>;
}

function AddressField({ label, value, onChange, fallback, testid }: { label: string; value: string; onChange: (value:string)=>void; fallback: string | null; testid:string }) {
  return <div className="field"><label>{label}</label><input className="input mono" value={value} onChange={e=>onChange(e.target.value)} placeholder={fallback || 'Enter a valid Stellar account address'} data-testid={testid} /><div className="field-help">{fallback ? `Connected wallet: ${shortenAddress(fallback)}. Leave blank to use it.` : 'No wallet connected. Provide an address or connect Freighter.'}</div></div>;
}
function TextField({ label, value, onChange, placeholder, testid }: { label:string; value:string; onChange:(value:string)=>void; placeholder:string; testid:string }) {
  return <div className="field"><label>{label}</label><input className="input mono" value={value} onChange={e=>onChange(e.target.value)} placeholder={placeholder} data-testid={testid} /></div>;
}

function DashboardOverview() {
  const { network, wallet, settings } = useAppState();
  const balanceContract = settings.contracts.account_ledger;
  const [balance, setBalance] = useState<Outcome | null>(null);
  const [lastReadAt, setLastReadAt] = useState('');
  useEffect(() => {
    let active = true;
    let inFlight = false;
    const refresh = async () => {
      if (!wallet || !isValidContractAddress(balanceContract)) {
        if (active) setBalance(null);
        return;
      }
      if (inFlight) return;
      inFlight = true;
      try {
        const value = await simulateContractRead({
          config: settings,
          sourceAddress: wallet,
          contractId: balanceContract,
          method: 'get_balance',
          args: [{name:'user',type:'address',value:wallet}],
        });
        if (active) {
          setBalance({kind:'read',text:toDisplayJson(value)});
          setLastReadAt(new Date().toLocaleTimeString());
        }
      } catch (error) {
        if (active) setBalance({kind:'error',text:error instanceof Error ? error.message : 'Live balance read failed.'});
      } finally { inFlight = false; }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 5_000);
    return () => { active = false; window.clearInterval(timer); };
  }, [wallet, balanceContract, settings.network, settings.rpcUrl, settings.customNetworkPassphrase]);
  return <div>
    <div className="callout" style={{marginBottom:15}}><div><div className="callout-label">Community power · verified state only</div><div className="callout-title">Your neighborhood, in real time</div><div className="callout-text">Balance, network health, and service state will appear when the configured Soroban contracts return live values. Nothing is estimated.</div></div><StatusTag tone={network ? 'ok' : 'warn'}>{network ? `LEDGER ${network.latestLedger}` : 'RPC NOT CONNECTED'}</StatusTag></div>
    <div className="grid cols-3" style={{marginBottom:15}}>
      <MetricCard icon={<Wallet/>} label="Energy credit balance" value={balance?.kind==='read' ? balance.text : '—'} note={balance?.kind==='read' ? `Live account-ledger read · refreshed every 5 seconds${lastReadAt ? ` · ${lastReadAt}` : ''}` : wallet && isValidContractAddress(balanceContract) ? 'Waiting for live account-ledger read' : 'Wallet and deployed account ledger required'} testid="metric-energy-balance" />
      <MetricCard icon={<Zap/>} label="Current service status" value="—" note="No service status read has been returned" testid="metric-service-status" />
      <MetricCard icon={<Activity/>} label="Latest meter interval" value="—" note="No telemetry is seeded or assumed" testid="metric-meter-interval" />
    </div>
    {balance?.kind==='error'&&<div className="notice error" style={{marginBottom:15}} data-testid="status-dashboard-balance-error">The five-second balance read failed: {balance.text}</div>}
  </div>;
}
function DashboardSecondary() {
  return <><div className="section-kicker">Community systems</div><div className="grid cols-2"><div className="card"><div className="card-header"><div><h2 className="card-title">Network pulse</h2><div className="card-subtitle">Current operational state from configured RPC.</div></div><span className="metric-icon"><Network /></span></div><EmptyState title="No topology snapshot loaded">Read live nodes in Network topology. This overview does not construct a synthetic neighborhood.</EmptyState></div><div className="card"><div className="card-header"><div><h2 className="card-title">Energy ledger</h2><div className="card-subtitle">Account-level chain data, not an estimate.</div></div><span className="metric-icon"><FileClock /></span></div><EmptyState title="No account history loaded">Fetch the configured meter or account contract from its dedicated view.</EmptyState></div></div></>;
}
function MetricCard({ icon, label, value, note, testid }: { icon:ReactNode;label:string;value:string;note:string;testid:string }) {
  return <div className="card" data-testid={testid}><div className="row"><span className="metric-label">{label}</span><span className="metric-icon">{icon}</span></div><div className="metric-value">{value}</div><div className="metric-note">{note}</div></div>;
}

function SettingsView({ updateSettings }: { updateSettings:(settings:SolNetSettings)=>void }) {
  const { settings, network, networkError, refreshNetwork, wallet } = useAppState();
  const [draft, setDraft] = useState<SolNetSettings>(()=>({...settings,contracts:{...settings.contracts}}));
  const [saved, setSaved] = useState(false);
  const [checkError, setCheckError] = useState('');
  const [checking, setChecking] = useState(false);
  useEffect(()=>setDraft({...settings,contracts:{...settings.contracts}}),[settings]);
  const setContract = (key:ContractKey,value:string) => setDraft(current=>({...current,contracts:{...current.contracts,[key]:value}}));
  const hasInvalid = CONTRACTS.some(contract => {
    const value=draft.contracts[contract.key];
    return !!value.trim() && !isValidContractAddress(value);
  });
  const checkDeployment = async (key:ContractKey) => {
    setChecking(true); setCheckError('');
    try { await verifyContractDeployment(draft.contracts[key],draft); setCheckError(`${CONTRACT_BY_KEY[key].label}: deployment exists on ${draft.network}.`); }
    catch(error) { setCheckError(error instanceof Error ? error.message : 'Deployment could not be verified.'); }
    finally { setChecking(false); }
  };
  const save = () => {
    if (hasInvalid) { setCheckError('Correct invalid contract addresses before saving.'); return; }
    try { new URL(draft.rpcUrl); } catch { setCheckError('Enter a valid RPC URL before saving.'); return; }
    if (draft.network !== 'standalone' && !draft.rpcUrl.startsWith('https://')) { setCheckError('Public networks require an HTTPS RPC URL.'); return; }
    updateSettings(draft); setSaved(true); setTimeout(()=>setSaved(false),2400);
  };
  return <div>
    <PageHeading page={titleFor('/settings')} actions={<button className="button" onClick={save} data-testid="button-save-settings">Save settings</button>} />
    <div className="grid cols-2" style={{alignItems:'start'}}>
      <section className="card">
        <div className="card-header"><div><h2 className="card-title">Network connection</h2><div className="card-subtitle">Settings are persisted in this browser. Environment config initializes defaults.</div></div><span className="metric-icon"><Cable /></span></div>
        <div className="field"><label>Stellar network</label><select className="select" value={draft.network} onChange={e=>setDraft(current=>{const nextNetwork=e.target.value as SolNetSettings['network'];const followsDefault=current.rpcUrl===NETWORKS[current.network].defaultRpcUrl;return {...current,network:nextNetwork,rpcUrl:followsDefault?NETWORKS[nextNetwork].defaultRpcUrl:current.rpcUrl};})} data-testid="select-network"><option value="testnet">Testnet</option><option value="mainnet">Mainnet</option><option value="standalone">Standalone</option></select></div>
        <div className="field"><label>Effective network passphrase</label><div className="input mono" style={{height:'auto',overflowWrap:'anywhere'}} data-testid="text-effective-passphrase">{getNetworkPassphrase(draft)}</div></div>
        <div className="field"><label>RPC endpoint</label><input className="input mono" value={draft.rpcUrl} onChange={e=>setDraft(s=>({...s,rpcUrl:e.target.value}))} placeholder="https://…" data-testid="input-rpc-url" /><span className="field-help">Public networks must use HTTPS. Standalone may use HTTP.</span></div>
        {draft.network==='standalone' && <div className="field"><label>Custom network passphrase (optional)</label><input className="input mono" value={draft.customNetworkPassphrase ?? ''} onChange={e=>setDraft(s=>({...s,customNetworkPassphrase:e.target.value}))} placeholder="Leave blank to use Standalone passphrase" data-testid="input-network-passphrase" /></div>}
        <div className="row" style={{justifyContent:'flex-start',flexWrap:'wrap'}}><button className="button" disabled={checking} onClick={async()=>{setChecking(true);await refreshNetwork();setChecking(false);}} data-testid="button-check-rpc"><RefreshCw size={13}/>{checking?'Checking…':'Check RPC health'}</button><StatusTag tone={network?'ok':networkError?'error':''}>{network?`LIVE · ledger ${network.latestLedger}`:networkError?'ERROR':'NOT CHECKED'}</StatusTag></div>
        {network && <div className="section-kicker" data-testid="status-rpc-details">Latest ledger {network.latestLedger} · {network.latencyMs} ms · checked {new Date(network.checkedAt).toLocaleTimeString()}</div>}
        {saved && <div className="notice neutral" style={{marginTop:12}} data-testid="status-settings-saved">Saved to this browser. RPC health will refresh with the new configuration.</div>}
        {checkError && <div className={`notice ${checkError.includes('exists')?'neutral':'error'}`} style={{marginTop:12}} data-testid="status-settings-validation">{checkError}</div>}
        {hasInvalid && <div className="notice error" style={{marginTop:12}}>At least one entered contract ID is invalid. Contract IDs are not examples: enter the real deployed ID or leave blank.</div>}
        <div className="divider" /><div className="metric-label">Wallet state</div><div style={{marginTop:8}} className="mono" data-testid="text-wallet-state">{wallet || 'No wallet connected'}</div>
      </section>
      <section className="card">
        <div className="card-header"><div><h2 className="card-title">Contract registry</h2><div className="card-subtitle">Only exact contract keys and methods supplied by the project registry are listed.</div></div><span className="metric-icon"><Blocks /></span></div>
        <div className="notice neutral" style={{marginBottom:14}}>No example contract IDs are prefilled. A valid address alone does not establish that a contract exists; verify each deployment against the selected network.</div>
        <div className="grid" style={{gap:9}}>
          {CONTRACTS.map(item=><div key={item.key} style={{borderTop:'1px solid hsl(var(--border))',paddingTop:10}}>
            <div className="row"><div><div className="card-title">{item.label}</div><div className="field-help mono">{item.key}</div></div><StatusTag tone={draft.contracts[item.key]&&isValidContractAddress(draft.contracts[item.key])?'ok':draft.contracts[item.key]?'error':'warn'}>{!draft.contracts[item.key]?'UNCONFIGURED':isValidContractAddress(draft.contracts[item.key])?'ADDRESS VALID':'INVALID ID'}</StatusTag></div>
            <input className="input mono" style={{marginTop:8}} value={draft.contracts[item.key]} onChange={e=>setContract(item.key,e.target.value)} placeholder="Paste real deployed contract ID" aria-label={`${item.label} contract ID`} data-testid={`input-contract-${item.key}`} />
            <div className="row" style={{marginTop:6}}><span className="field-help">{item.methods.map(m=>m.name).join(' · ')}</span><button className="button ghost" style={{minHeight:29,padding:'0 8px',fontSize:9}} disabled={!draft.contracts[item.key]||!isValidContractAddress(draft.contracts[item.key])||checking} onClick={()=>void checkDeployment(item.key)} data-testid={`button-verify-${item.key}`}>Verify deployment</button></div>
          </div>)}
        </div>
      </section>
    </div>
  </div>;
}

function DeveloperView() {
  const { settings, wallet } = useAppState();
  const [contractKey,setContractKey]=useState<ContractKey>('grid_topology');
  const contract=CONTRACT_BY_KEY[contractKey];
  const [methodName,setMethodName]=useState(contract.methods[0]?.name ?? '');
  const selected=contract.methods.find(method=>method.name===methodName);
  const [args,setArgs]=useState<ContractArgument[]>(selected?defaultArguments(selected):[]);
  const [result,setResult]=useState<Outcome|null>(null);
  const [review,setReview]=useState(false);
  const [busy,setBusy]=useState(false);
  const [source,setSource]=useState('');
  const contractId=settings.contracts[contractKey];
  useEffect(()=>{ const next=CONTRACT_BY_KEY[contractKey];setMethodName(next.methods[0]?.name??''); },[contractKey]);
  useEffect(()=>{ const method=contract.methods.find(entry=>entry.name===methodName);setArgs(method?defaultArguments(method):[]);setResult(null);setReview(false); },[contract,methodName]);
  const method=selected;
  const methodNeedsTypedArgs=method?.access==='unknown'||!method?.args;
  const valid=!!wallet && isValidContractAddress(contractId) && !!method && (!methodNeedsTypedArgs||args.length>0) && args.every(arg=>arg.name.trim() && arg.value!=='' && (typeof arg.value!=='string'||arg.value.trim()));
  const sourceMatchesSigner=!source.trim()||source.trim()===wallet;
  const updateArg=(index:number,patch:Partial<ContractArgument>)=>setArgs(current=>current.map((arg,i)=>i===index?{...arg,...patch}:arg));
  const addArg=()=>setArgs(current=>[...current,{name:`arg_${current.length+1}`,type:'string',value:''}]);
  const invoke=async(write:boolean)=>{
    setBusy(true);setResult(null);setReview(false);
    try {
      await verifyContractDeployment(contractId,settings);
      if(write){
        const tx=await submitContractTransaction({config:settings,sourceAddress:source.trim()||wallet!,contractId,method:method!.name,args});
        setResult({kind:'tx',text:tx.status==='SUCCESS'?'RPC confirmed this transaction on-chain.':'RPC accepted this transaction and it remains pending.',hash:tx.hash,status:tx.status});
      } else {
        await assertWalletNetwork(settings);
        const value=await simulateContractRead({config:settings,sourceAddress:source.trim()||wallet!,contractId,method:method!.name,args});
        setResult({kind:'read',text:toDisplayJson(value)});
      }
    }catch(error){setResult({kind:'error',text:error instanceof Error?error.message:'RPC request failed.'});}
    finally{setBusy(false);}
  };
  const firstContract=contract;
  return <div>
    <PageHeading page={titleFor('/developer-tools')} />
    <div className="grid cols-2" style={{alignItems:'start'}}>
      <section className="card">
        <div className="card-header"><div><h2 className="card-title">RPC invocation</h2><div className="card-subtitle">The contract registry defines known signatures; unspecified ABI must be entered manually.</div></div><span className="metric-icon"><Wrench /></span></div>
        <div className="field"><label>Contract</label><select className="select" value={contractKey} onChange={e=>setContractKey(e.target.value as ContractKey)} data-testid="select-developer-contract">{CONTRACTS.map(item=><option key={item.key} value={item.key}>{item.label} · {item.key}</option>)}</select></div>
        <div className="field"><label>Contract ID</label><div className="input mono" data-testid="text-developer-contract-id">{contractId||'No contract ID configured'}</div><div className="field-help"><Link href="/settings" className="inline-code" data-testid="link-developer-settings">Configure contract IDs</Link></div></div>
        <div className="field"><label>Method</label><select className="select" value={methodName} onChange={e=>setMethodName(e.target.value)} data-testid="select-developer-method">{contract.methods.map(entry=><option key={entry.name} value={entry.name}>{entry.name} · {entry.access}</option>)}</select></div>
         <div className="field"><label>Source address</label><input className="input mono" value={source} onChange={e=>setSource(e.target.value)} placeholder={wallet||'Connect wallet to simulate or invoke'} data-testid="input-developer-source" /><div className="field-help">Reads may use another source account. Signed transactions always use the connected Freighter account.</div></div>
        <div className="row"><div className="section-kicker" style={{margin:'8px 0'}}>Arguments</div>{methodNeedsTypedArgs&&<button className="button ghost" onClick={addArg} data-testid="button-add-argument">Add typed argument</button>}</div>
        {methodNeedsTypedArgs&&<div className="notice" style={{marginBottom:10}}>This method has no verified argument list. Define each argument name, Soroban type, and value explicitly. Do not infer an ABI.</div>}
        {args.length===0?<EmptyState title="No arguments declared">Zero arguments are present in the supplied registry for this method.</EmptyState>:args.map((arg,index)=><div key={index} className="grid cols-3" style={{gap:7,marginBottom:8}}>
          <input className="input mono" value={arg.name} onChange={e=>updateArg(index,{name:e.target.value})} aria-label={`Argument ${index+1} name`} placeholder="Argument name" data-testid={`input-argument-name-${index}`} />
          <select className="select mono" value={arg.type} onChange={e=>updateArg(index,{type:e.target.value as SorobanArgumentType,value:''})} aria-label={`Argument ${index+1} type`} data-testid={`select-argument-type-${index}`}>{TYPE_OPTIONS.map(type=><option key={type} value={type}>{type}</option>)}</select>
          {arg.type==='bool'?<select className="select mono" value={String(arg.value)} onChange={e=>updateArg(index,{value:e.target.value==='true'})} aria-label={`Argument ${index+1} value`} data-testid={`input-argument-value-${index}`}><option value="false">false</option><option value="true">true</option></select>:<input className="input mono" value={String(arg.value)} onChange={e=>updateArg(index,{value:e.target.value})} aria-label={`Argument ${index+1} value`} placeholder="Value" data-testid={`input-argument-value-${index}`} />}
        </div>)}
        <div className="row" style={{justifyContent:'flex-start',flexWrap:'wrap',marginTop:13}}>
          <button className="button secondary" disabled={!valid||busy||method?.access==='write'} onClick={()=>void invoke(false)} data-testid="button-simulate-read"><Activity size={13}/>{busy?'Working…':'Simulate read'}</button>
          <button className="button" disabled={!valid||!sourceMatchesSigner||busy||method?.access==='read'} onClick={()=>setReview(true)} data-testid="button-review-invocation">Review transaction</button>
        </div>
        {!sourceMatchesSigner&&<div className="notice" style={{marginTop:9}}>For safety, a signed transaction must use the connected Freighter account as its source.</div>}
        {review&&<div className="notice neutral" style={{marginTop:13}} data-testid="panel-developer-review">
          <b>Final confirmation before Freighter</b><div className="divider" /><div className="field-help">Network · {settings.network}</div><div className="field-help">Target · <span className="mono">{contractId}</span></div><div className="field-help">Method · <span className="mono">{method?.name}</span></div><pre className="result-box" style={{maxHeight:160,margin:'9px 0'}}>{toDisplayJson(args)}</pre>
           <div className="row" style={{justifyContent:'flex-start'}}><button className="button" disabled={busy||!sourceMatchesSigner} onClick={()=>void invoke(true)} data-testid="button-sign-invocation">{busy?'Awaiting RPC…':'Confirm & invoke with Freighter'}</button><button className="button ghost" onClick={()=>setReview(false)} data-testid="button-cancel-invocation">Cancel</button></div>
        </div>}
        {firstContract && !isValidContractAddress(contractId)&&<div className="notice" style={{marginTop:12}}>This method cannot run until a valid Soroban contract ID is configured for the selected network.</div>}
      </section>
      <section className="card">
        <div className="card-header"><div><h2 className="card-title">Response & diagnostics</h2><div className="card-subtitle">No response means no verified contract data.</div></div><StatusTag tone={result?.kind==='read'||result?.kind==='tx'?'ok':result?.kind==='error'?'error':''}>{result?result.kind==='error'?'ERROR':result.kind==='tx'?result.status??'SUBMITTED':'RPC RESULT':'IDLE'}</StatusTag></div>
        {busy?<div className="skeleton-stack" data-testid="loading-developer-result"><div className="skeleton-line"/><div className="skeleton-line"/><div className="skeleton-line"/></div>:result?.kind==='error'?<div className="notice error" data-testid="status-developer-error">{result.text}</div>:result?.kind==='read'?<pre className="result-box" data-testid="data-developer-response">{result.text}</pre>:result?.kind==='tx'?<><div className="notice neutral" data-testid="status-developer-transaction">{result.text}</div><div className="section-kicker">Hash</div><div className="result-box" data-testid="text-developer-hash">{result.hash}</div></>:<EmptyState title="No RPC call made">Select an explicitly defined interface and make a live RPC request to populate diagnostics.</EmptyState>}
        <div className="divider" /><div className="row"><span className="metric-label">Interface source</span><span className="inline-code">src/lib/contracts.ts</span></div><div className="divider" /><p className="field-help" style={{lineHeight:1.7}}>Simulation is not proof of a successful transaction. A write is reported as pending or confirmed only from the helper's RPC response. ABI coverage is limited to the supplied registry; no Rust/WASM interface has been inferred.</p>
      </section>
    </div>
  </div>;
}

export default App;
