const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const SVG_NS = "http://www.w3.org/2000/svg";

const SPIN = { low: "2s", medium: "1.1s", auto: "1.1s", high: "0.7s", turbo: "0.45s" };
const WINDFREE_SPIN = "4s";
const SETPOINT_DEBOUNCE_MS = 700;
const TOAST_MS = 4000;

const CONNECTION = {
  connecting: ["Conectando…", ""],
  online: ["Online", "online"],
  noToken: ["Sem token", "warning"],
  noDevice: ["Sem aparelho", "warning"],
  unauthorized: ["Token expirado", "error"],
  failed: ["Sem conexão", "error"],
};

const BANNER = {
  noToken: "Cole um token do SmartThings para começar.",
  noDevice: "Escolha qual ar-condicionado controlar.",
  unauthorized: "O token do SmartThings expirou. Gere um novo e salve em Ajustes.",
};

// Choice groups: status field with the current value, field with the options.
const CHOICES = {
  mode: ["mode", "supportedModes"],
  fanMode: ["fanMode", "supportedFanModes"],
  oscillation: ["oscillation", "supportedOscillations"],
  optionalMode: ["optionalMode", "supportedOptionalModes"],
};

// What each option does, from the Samsung WindFree user manuals (DB68-11414A, DB68-12013A).
const DESCRIPTIONS = {
  mode: {
    auto: "Resfria ou aquece sozinho até chegar à temperatura programada. Perto dela, o aparelho passa a controlar a ventilação e a direção do vento.",
    cool: "Resfria o ambiente. Para gelar mais rápido, baixe a temperatura e aumente a ventilação; para economizar, faça o contrário.",
    dry: "Tira a umidade do ar em dias chuvosos ou abafados. Esfria um pouco como efeito colateral e nunca aquece. Ventilação sempre automática.",
    wind: "Funciona como um ventilador: só circula o ar, sem resfriar nem aquecer. A unidade externa fica parada. Bom para secar o aparelho antes de um período sem uso.",
    heat: "Aquece o ambiente. No começo, a ventilação pode ficar parada por 3 a 5 minutos para não soprar ar frio.",
  },
  fanMode: {
    auto: "O aparelho escolhe a velocidade conforme a distância até a temperatura programada. É a única opção nos modos Automático e Desumidificar.",
    low: "Velocidade baixa: menos barulho e menos vento, resfria mais devagar.",
    medium: "Velocidade intermediária, equilíbrio entre conforto e ruído.",
    high: "Velocidade alta, espalha o ar mais rápido pelo ambiente.",
    turbo: "Velocidade máxima, para resfriar ou aquecer o mais rápido possível.",
  },
  oscillation: {
    fixed: "As aletas ficam paradas e o vento sai sempre na mesma direção.",
    vertical: "A aleta sobe e desce, espalhando o ar entre o teto e o chão.",
    horizontal: "As aletas vão para a esquerda e para a direita, espalhando o ar pela largura do ambiente.",
    all: "Move as aletas nos dois sentidos para espalhar o ar pelo ambiente todo.",
  },
  optionalMode: {
    off: "Nenhuma função especial: o aparelho segue só o modo e a ventilação escolhidos.",
    sleep: "good'sleep: ajusta a temperatura durante a noite para economizar enquanto você dorme. Só em Resfriar ou Aquecer; recomendado 25 a 27 °C resfriando.",
    quiet: "Reduz o barulho do aparelho. Só em Resfriar ou Aquecer; a ventilação fica travada enquanto estiver ativo.",
    smart: "Smart Saver: modo de economia de energia que restringe a temperatura programada para o aparelho gastar menos.",
    speed: "Rápido: resfria ou aquece o mais rápido possível. Só em Resfriar ou Aquecer; temperatura e ventilação ficam travadas enquanto estiver ativo.",
    windFree: "WindFree: o ar sai pelos milhares de microfuros do painel, sem vento direto no corpo. Funciona em Resfriar, Desumidificar e Ventilar; ideal entre 24 e 26 °C.",
    windFreeSleep: "WindFree com good'sleep: o resfriamento suave pelos microfuros somado ao ajuste noturno de temperatura do good'sleep.",
  },
};

let labels = {};
let snapshot = null;
let settings = null;
let pendingSetpoint = null;
let setpointTimer = null;
let historyRange = 3600;
let toastTimer = null;

const label = (value) => labels[value] ?? value;
const fmt = (value, unit = "", digits = 0) => (value == null ? "--" : `${value.toFixed(digits)}${unit}`);
const watts = (w) => (w == null ? "--" : w >= 1000 ? `${(w / 1000).toFixed(2)} kW` : `${w.toFixed(0)} W`);

function toast(message) {
  const el = $("toast");
  el.textContent = message;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.hidden = true), TOAST_MS);
}

async function send(kind, value) {
  try {
    await invoke("send_command", { command: { kind, value } });
  } catch (error) {
    toast(`Comando não enviado: ${error}`);
  }
}

/* ---------- Control ---------- */

function drawHoles() {
  const group = $("holes");
  for (let radius = 66; radius <= 96; radius += 7) {
    const count = Math.round((2 * Math.PI * radius) / 7);
    for (let i = 0; i < count; i++) {
      const angle = (i / count) * 2 * Math.PI;
      const hole = document.createElementNS(SVG_NS, "circle");
      hole.setAttribute("cx", (radius * Math.cos(angle)).toFixed(1));
      hole.setAttribute("cy", (radius * Math.sin(angle)).toFixed(1));
      hole.setAttribute("r", "1.6");
      hole.setAttribute("class", "hole");
      // Rings light up from the inside out, like air leaving the panel.
      hole.style.setProperty("--delay", `${((radius - 66) / 30) * 1.2}s`);
      group.append(hole);
    }
  }
}

function describe(container, value) {
  container.nextElementSibling.textContent = DESCRIPTIONS[container.dataset.kind]?.[value] ?? "";
}

const currentChoice = (container) => snapshot?.status?.[CHOICES[container.dataset.kind][0]];

function renderChoices(container, options, current) {
  const key = options.join("|");
  if (container.dataset.options !== key) {
    container.dataset.options = key;
    container.replaceChildren(
      ...options.map((option) => {
        const button = document.createElement("button");
        button.textContent = label(option);
        button.dataset.value = option;
        return button;
      }),
    );
  }
  for (const button of container.children) {
    button.setAttribute("aria-pressed", String(button.dataset.value === current));
  }
  // Keep a hover preview in place when a poll refreshes the status.
  if (!container.matches(":hover")) describe(container, current);
}

function render(next) {
  snapshot = next;
  const s = next.status;
  const offline = next.connection === "online" && s && !s.online;
  const [text, state] = offline ? ["Aparelho offline", "warning"] : CONNECTION[next.connection];
  $("connection").textContent = text;
  $("connection").dataset.state = state;

  const bannerText = BANNER[next.connection] ?? (next.connection === "failed" ? next.error : null);
  $("banner").hidden = !bannerText;
  $("banner-text").textContent = bannerText ?? "";

  const running = Boolean(s?.power && s.online);
  const windfree = running && Boolean(s.optionalMode?.startsWith("windFree"));
  document.body.dataset.mode = s?.power ? (s.mode ?? "off") : "off";
  document.body.classList.toggle("on", Boolean(s?.power));
  document.body.classList.toggle("running", running);
  document.body.classList.toggle("windfree", windfree);
  document.body.style.setProperty("--spin", windfree ? WINDFREE_SPIN : (SPIN[s?.fanMode] ?? SPIN.auto));

  $("temperature").textContent = fmt(s?.temperature, "°");
  $("humidity").textContent = fmt(s?.humidity, "%");
  $("power-watts").textContent = !s ? "--" : s.power ? watts(s.powerWatts) : "desligado";
  $("filter").textContent =
    s?.filterUsage == null ? "--" : `${s.filterUsage}% ${s.filterStatus === "normal" ? "ok" : "limpar"}`;
  $("setpoint").textContent = fmt(pendingSetpoint ?? s?.setpoint, "°");
  $("power").textContent = s?.power ? "Desligar" : "Ligar";

  for (const container of document.querySelectorAll(".choices")) {
    const [currentKey, optionsKey] = CHOICES[container.dataset.kind];
    renderChoices(container, s?.[optionsKey] ?? [], s?.[currentKey]);
  }
  for (const button of document.querySelectorAll("#tab-control button")) {
    button.disabled = !s;
  }
  if (!$("tab-metrics").hidden) loadHistory();
}

function stepSetpoint(delta) {
  const s = snapshot?.status;
  const base = pendingSetpoint ?? s?.setpoint;
  if (base == null) return;
  pendingSetpoint = Math.min(s.setpointMax, Math.max(s.setpointMin, base + delta));
  $("setpoint").textContent = fmt(pendingSetpoint, "°");
  // Rapid clicks become a single command.
  clearTimeout(setpointTimer);
  setpointTimer = setTimeout(async () => {
    await send("setpoint", pendingSetpoint);
    pendingSetpoint = null;
    render(snapshot);
  }, SETPOINT_DEBOUNCE_MS);
}

/* ---------- Metrics ---------- */

const CHART_HEIGHT = 170;
const PAD = { left: 56, right: 12, top: 12, bottom: 24 };
const CHARTS = [
  {
    id: "chart-temperature",
    unit: "°",
    series: [
      { key: "temperature", className: "ambient-line", name: "ambiente" },
      { key: "setpoint", className: "setpoint-line", name: "programada" },
    ],
  },
  { id: "chart-humidity", unit: "%", series: [{ key: "humidity", className: "humidity-line", name: "umidade" }] },
  { id: "chart-power", unit: " W", series: [{ key: "powerWatts", className: "power-line", name: "consumo" }] },
];
let lastHistory = null;

const longRange = () => historyRange > 86400;
const dateLabel = (ts) => new Date(ts * 1000).toLocaleDateString("pt-BR", { day: "2-digit", month: "2-digit" });
const timeLabel = (ts) => new Date(ts * 1000).toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });
const axisLabel = (ts) => (longRange() ? dateLabel(ts) : timeLabel(ts));
const readoutLabel = (ts) => (longRange() ? `${dateLabel(ts)} ${timeLabel(ts)}` : timeLabel(ts));

function svgEl(name, attrs, text) {
  const el = document.createElementNS(SVG_NS, name);
  for (const [key, value] of Object.entries(attrs)) el.setAttribute(key, value);
  if (text != null) el.textContent = text;
  return el;
}

/** Readings further apart than this belong to separate runs (the app was closed in between). */
function gapThreshold(points) {
  const diffs = points.slice(1).map((p, i) => p.ts - points[i].ts).sort((a, b) => a - b);
  const median = diffs[Math.floor(diffs.length / 2)] ?? 0;
  return Math.max(median, settings?.pollIntervalSecs ?? 30) * 3;
}

function segments(points, key, maxGap) {
  const runs = [];
  let run = [];
  let previous = null;
  for (const p of points) {
    if (p[key] == null || (previous != null && p.ts - previous > maxGap)) {
      if (run.length) runs.push(run);
      run = [];
    }
    if (p[key] != null) run.push(p);
    previous = p.ts;
  }
  if (run.length) runs.push(run);
  return runs;
}

function setReadout(chart, point) {
  const output = $(chart.id).closest("figure").querySelector(".readout");
  if (!point) {
    output.textContent = "";
    return;
  }
  const value = (v) => (chart.unit === " W" ? watts(v) : fmt(v, chart.unit));
  const parts = chart.series
    .filter((s) => point[s.key] != null)
    .map((s) => (chart.series.length > 1 ? `${s.name} ${value(point[s.key])}` : value(point[s.key])));
  output.textContent = `${readoutLabel(point.ts)}: ${parts.join(", ")}`;
}

const hasValue = (chart, p) => chart.series.some((s) => p[s.key] != null);

function drawChart(chart, points) {
  const svg = $(chart.id);
  const width = Math.max(svg.clientWidth, 240);
  const H = CHART_HEIGHT;
  svg.setAttribute("viewBox", `0 0 ${width} ${H}`);
  chart.state = null;
  const values = points.flatMap((p) => chart.series.map((s) => p[s.key])).filter((v) => v != null);
  if (values.length === 0) {
    svg.replaceChildren(svgEl("text", { x: width / 2, y: H / 2, class: "empty" }, "Sem leituras neste período"));
    setReadout(chart, null);
    return;
  }

  // The x axis always spans the selected period, so switching ranges is visible.
  const t1 = Math.floor(Date.now() / 1000);
  const t0 = t1 - historyRange;
  const dataMin = Math.min(...values);
  const dataMax = Math.max(...values);
  const spread = Math.max(dataMax - dataMin, 2);
  const lo = (dataMin + dataMax) / 2 - spread * 0.62;
  const hi = (dataMin + dataMax) / 2 + spread * 0.62;
  const x = (ts) => PAD.left + ((ts - t0) / (t1 - t0)) * (width - PAD.left - PAD.right);
  const y = (v) => PAD.top + (1 - (v - lo) / (hi - lo)) * (H - PAD.top - PAD.bottom);
  const digits = chart.unit !== " W" && dataMax - dataMin < 4 ? 1 : 0;
  const ticks = dataMax === dataMin ? [dataMin] : [dataMin, (dataMin + dataMax) / 2, dataMax];

  const children = [];
  for (const tick of ticks) {
    children.push(
      svgEl("line", { x1: PAD.left, x2: width - PAD.right, y1: y(tick), y2: y(tick), class: "grid" }),
      svgEl("text", { x: PAD.left - 8, y: y(tick) + 4, class: "axis", "text-anchor": "end" }, fmt(tick, chart.unit, digits)),
    );
  }
  for (const [ts, anchor] of [[t0, "start"], [(t0 + t1) / 2, "middle"], [t1, "end"]]) {
    children.push(svgEl("text", { x: x(ts), y: H - 6, class: "axis", "text-anchor": anchor }, axisLabel(ts)));
  }
  const gap = gapThreshold(points);
  for (const s of chart.series) {
    for (const run of segments(points, s.key, gap)) {
      // SmartThings reports discrete values, so hold each reading until the next one.
      let d = run
        .map((p, i) => (i ? `H${x(p.ts).toFixed(1)} V${y(p[s.key]).toFixed(1)}` : `M${x(p.ts).toFixed(1)} ${y(p[s.key]).toFixed(1)}`))
        .join(" ");
      if (run.length === 1) d += " l0.01 0"; // a lone reading renders as a dot thanks to round caps
      children.push(svgEl("path", { d, class: `line ${s.className}` }));
    }
  }
  svg.replaceChildren(...children);
  chart.state = { points, x, y, t0, t1, width, gap };
  setReadout(chart, points.findLast((p) => hasValue(chart, p)));
}

function hoverChart(chart, event) {
  const svg = $(chart.id);
  svg.querySelector(".hover")?.remove();
  const st = chart.state;
  if (!st) return;
  if (!event) {
    setReadout(chart, st.points.findLast((p) => hasValue(chart, p)));
    return;
  }
  const rect = svg.getBoundingClientRect();
  const ratio = (event.clientX - rect.left - PAD.left) / (rect.width - PAD.left - PAD.right);
  const ts = st.t0 + ratio * (st.t1 - st.t0);
  let nearest = null;
  for (const p of st.points) {
    if (hasValue(chart, p) && (!nearest || Math.abs(p.ts - ts) < Math.abs(nearest.ts - ts))) nearest = p;
  }
  if (!nearest || Math.abs(nearest.ts - ts) > st.gap) return;
  const group = svgEl("g", { class: "hover" });
  group.append(svgEl("line", { x1: st.x(nearest.ts), x2: st.x(nearest.ts), y1: PAD.top, y2: CHART_HEIGHT - PAD.bottom, class: "guide" }));
  for (const s of chart.series) {
    if (nearest[s.key] == null) continue;
    group.append(svgEl("circle", { cx: st.x(nearest.ts), cy: st.y(nearest[s.key]), r: 4, class: `marker ${s.className}` }));
  }
  svg.append(group);
  setReadout(chart, nearest);
}

function renderHistory() {
  if (!lastHistory) return;
  const { points, summary } = lastHistory;
  const range = (stats, unit) => (stats ? `${fmt(stats.min, unit)} a ${fmt(stats.max, unit)}` : "sem leituras");
  $("sum-temp").textContent = fmt(summary.temperature?.avg, "°", 1);
  $("sum-temp-range").textContent = range(summary.temperature, "°");
  $("sum-humidity").textContent = fmt(summary.humidity?.avg, "%");
  $("sum-humidity-range").textContent = range(summary.humidity, "%");
  $("sum-energy").textContent = summary.energyWh == null ? "--" : `${(summary.energyWh / 1000).toFixed(2)} kWh`;
  $("sum-on").textContent = points.length ? `${Math.round(summary.onRatio * 100)}%` : "--";
  for (const chart of CHARTS) drawChart(chart, points);
}

async function loadHistory() {
  try {
    lastHistory = await invoke("get_history", { rangeSecs: historyRange });
    renderHistory();
  } catch (error) {
    toast(`Histórico indisponível: ${error}`);
  }
}

/* ---------- Settings ---------- */

const getPath = (obj, path) => path.split(".").reduce((o, k) => o?.[k], obj);
function setPath(obj, path, value) {
  const keys = path.split(".");
  const last = keys.pop();
  keys.reduce((o, k) => o[k], obj)[last] = value;
}

function fillSettings() {
  $("device-name").textContent = settings.deviceLabel ?? "Ar-condicionado";
  for (const input of $("settings-form").querySelectorAll("input[name]")) {
    const value = getPath(settings, input.name);
    if (input.type === "checkbox") input.checked = Boolean(value);
    else if (input.type === "radio") input.checked = input.value === value;
    else input.value = value ?? "";
  }
  const select = $("device");
  if (settings.deviceId && ![...select.options].some((o) => o.value === settings.deviceId)) {
    select.add(new Option(settings.deviceLabel ?? settings.deviceId, settings.deviceId));
  }
  select.value = settings.deviceId ?? "";
}

function fillDevices(devices) {
  const select = $("device");
  select.replaceChildren(new Option("Nenhum selecionado", ""));
  for (const device of devices.filter((d) => d.isAirConditioner)) {
    select.add(new Option(device.label, device.id));
  }
  fillSettings();
}

async function loadDevices() {
  try {
    fillDevices(await invoke("list_devices"));
  } catch {
    // Without a valid token the saved device stays listed on its own.
  }
}

async function saveSettings() {
  const next = structuredClone(settings);
  for (const input of $("settings-form").querySelectorAll("input[name]")) {
    if (input.type === "checkbox") setPath(next, input.name, input.checked);
    else if (input.type === "radio") {
      if (input.checked) setPath(next, input.name, input.value);
    } else setPath(next, input.name, input.value === "" ? null : Number(input.value));
  }
  const select = $("device");
  next.deviceId = select.value || null;
  next.deviceLabel = select.value ? select.selectedOptions[0].textContent : null;
  next.pollIntervalSecs ??= settings.pollIntervalSecs;
  next.retentionDays ??= settings.retentionDays;
  try {
    settings = await invoke("save_settings", { settings: next });
    fillSettings();
    $("settings-status").textContent = "Ajustes salvos.";
  } catch (error) {
    $("settings-status").textContent = `Ajustes não salvos: ${error}`;
  }
}

async function saveToken(event) {
  event.preventDefault();
  const token = $("token").value.trim();
  if (!token) return;
  try {
    const devices = await invoke("save_token", { token });
    $("token").value = "";
    settings = await invoke("get_settings");
    fillDevices(devices);
    toast("Token salvo. Conectando ao ar-condicionado…");
  } catch (error) {
    toast(`Token não salvo: ${error}`);
  }
}

/* ---------- Navigation ---------- */

function showTab(name) {
  for (const tab of document.querySelectorAll("[data-tab]")) {
    tab.setAttribute("aria-selected", String(tab.dataset.tab === name));
  }
  for (const panel of document.querySelectorAll("[role=tabpanel]")) {
    panel.hidden = panel.id !== `tab-${name}`;
  }
  if (name === "metrics") loadHistory();
  if (name === "settings") loadDevices();
}

function bindEvents() {
  for (const tab of document.querySelectorAll("[data-tab]")) {
    tab.addEventListener("click", () => showTab(tab.dataset.tab));
  }
  $("banner-action").addEventListener("click", () => showTab("settings"));
  $("power").addEventListener("click", () => send("power", !snapshot?.status?.power));
  $("warmer").addEventListener("click", () => stepSetpoint(1));
  $("cooler").addEventListener("click", () => stepSetpoint(-1));
  for (const container of document.querySelectorAll(".choices")) {
    container.addEventListener("click", (event) => {
      const value = event.target.closest("button")?.dataset.value;
      if (value) send(container.dataset.kind, value);
    });
    // Hovering or focusing an option previews what it does; leaving shows the active one again.
    const preview = (event) => describe(container, event.target.closest("button")?.dataset.value ?? currentChoice(container));
    const restore = () => describe(container, currentChoice(container));
    container.addEventListener("mouseover", preview);
    container.addEventListener("focusin", preview);
    container.addEventListener("mouseleave", restore);
    container.addEventListener("focusout", restore);
  }
  for (const button of document.querySelectorAll("[data-range]")) {
    button.addEventListener("click", () => {
      historyRange = Number(button.dataset.range);
      for (const b of document.querySelectorAll("[data-range]")) b.setAttribute("aria-pressed", String(b === button));
      loadHistory();
    });
  }
  $("token-form").addEventListener("submit", saveToken);
  $("open-token-page").addEventListener("click", () => invoke("open_token_page"));
  $("settings-form").addEventListener("change", saveSettings);
  $("settings-form").addEventListener("submit", (event) => event.preventDefault());
  // The theme itself is applied natively from Rust; this only flips the saved choice.
  $("theme-toggle").addEventListener("click", () => {
    const next = matchMedia("(prefers-color-scheme: dark)").matches ? "light" : "dark";
    document.querySelector(`input[name="appearance"][value="${next}"]`).checked = true;
    saveSettings();
  });
  for (const chart of CHARTS) {
    const svg = $(chart.id);
    svg.addEventListener("pointermove", (event) => hoverChart(chart, event));
    svg.addEventListener("pointerleave", () => hoverChart(chart, null));
  }
  let resizeFrame = 0;
  new ResizeObserver(() => {
    cancelAnimationFrame(resizeFrame);
    resizeFrame = requestAnimationFrame(renderHistory);
  }).observe($("charts"));
}

async function init() {
  drawHoles();
  bindEvents();
  [labels, settings] = await Promise.all([invoke("get_labels"), invoke("get_settings")]);
  fillSettings();
  render(await invoke("get_snapshot"));
  // The backend's "navigate" can fire before this listener exists, so first run checks here.
  if (!settings.deviceId) showTab("settings");
  await listen("snapshot", (event) => render(event.payload));
  await listen("navigate", (event) => showTab(event.payload));
}

init();
