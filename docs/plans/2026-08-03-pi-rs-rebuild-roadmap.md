# Implementation Plan

## Goal

С нуля построить проверяемый Rust-вариант `pi`, совместимый с зафиксированным upstream-контрактом, начиная с безопасного hermetic vertical slice и последовательно добавляя persistence, providers, tools, headless-протоколы, TUI и расширяемость через явно версионированные границы.

## Продуктовая цель и compatibility policy

### Зафиксированный upstream baseline

План ориентируется на локальный upstream:

- Repository: `/data/data/com.termux/files/home/pi-mono`
- Commit: `f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee`
- Coding-agent package: `@earendil-works/pi-coding-agent`
- Package version: `0.83.0`
- Contract root: `/data/data/com.termux/files/home/pi-mono/packages/coding-agent`
- Текущий исходный `pi-rs` baseline: commit `c0cd3dd76efee2ee6d2f9710eaeb0d86bd156109`

Commit SHA, а не плавающая ветка `main`, является compatibility anchor. Версия `0.83.0` служит удобной меткой, но при расхождении package version и содержимого репозитория приоритет имеет pinned commit.

### Продуктовая цель

Первый поддерживаемый релиз должен предоставлять:

1. Безопасный локальный coding-agent CLI с реальным provider/tool/session loop.
2. Точные и проверяемые контракты для session JSONL, print/JSON/RPC modes, settings, trust и Agent Skills относительно pinned upstream.
3. Честно ограниченную provider matrix: provider считается поддержанным только после общего offline contract suite.
4. Рабочую TUI поверх того же runtime, который используется headless-режимами.
5. Версионированные Rust-specific отклонения там, где прямое upstream-совпадение невозможно.
6. Hermetic CI и staged release без live credentials и сетевой зависимости.

### Зафиксированный профиль первого релиза

Первый compatibility release **включает**: session v3 valid-document compatibility, trust-aware settings/system/context loading, один API family после отдельной ADR и contract suite, bounded tools с upstream-style authorization, text/JSON/RPC over local stdio и TUI поверх общего runtime.

Первый release **откладывает**: ACP, executable extensions, broad provider matrix, full upstream theme/UI parity и package-manager distribution. Текущий sandbox не является частью product path: до отдельной ADR он доступен только как `experimental-unsafe-sandbox`, выключен в CI/release артефактах и сопровождается явным предупреждением.

Release-blocking ADRs: provider family/evidence, tool authorization, trust identity/precedence, RPC subset, sandbox quarantine, ACP defer, extension defer, migration recovery model и supported targets. Любая незакрытая ADR блокирует release, если нет явного waiver с owner и сроком.

### Уровни совместимости

В `docs/compatibility/upstream-policy.md` должна быть заведена матрица со следующими статусами:

- **Wire-compatible** — JSON/JSONL может обмениваться с pinned upstream без потери значимых полей.
- **Behavior-compatible** — одинаковые входы дают эквивалентное публичное поведение, допускаются внутренние отличия.
- **Mapped** — Rust предоставляет документированное соответствие, но не тот же wire/runtime ABI.
- **Rust extension** — дополнительная возможность, отсутствующая в upstream.
- **Unsupported** — возможность сознательно не реализуется в текущем релизе.

Правила:

- Session JSONL v1/v2/v3, upstream RPC subset, settings/trust и `SKILL.md` могут получить статус `wire-compatible` только после cross-reader/golden tests.
- Provider transport считается `behavior-compatible` отдельно для каждой API family и модели поведения: messages, tools, thinking, images, usage, stop reasons, streaming, timeout, cancellation, retry.
- TUI сравнивается по user scenarios, terminal lifecycle и commands, но не по пиксельному совпадению.
- Rust subprocess extension ABI не называется совместимым с upstream TypeScript/jiti extensions.
- Sandbox, extension ABI, ACP и широкая provider parity имеют собственные decision gates и не считаются подразумеваемой частью базового runtime.
- Обновление upstream требует нового manifest с SHA, provenance hashes и compatibility diff; нельзя молча передвинуть baseline на новый `main`.
- README и `pi --help` могут заявлять только те строки матрицы, которые прошли соответствующий exit gate.

## Архитектурная стратегия

Новая реализация создаётся рядом с текущими модулями и заменяет их только через контролируемый cutover. Текущий `src/` не используется как архитектурный шаблон: из него разрешено переносить только поведение, подтверждённое fixtures/tests.

Целевой workspace:

```text
pi-core         canonical messages, model/tool/usage/error types
pi-session      session schema, repository, tree, migrations, context projection
pi-provider     provider-neutral request/stream contracts and registry interfaces
pi-tools        tool definitions, schemas, execution context and registry
pi-agent        bounded, cancellable agent state machine
pi-resources    settings, trust, skills, prompts and system/context loading
pi-protocol     headless JSON events, RPC and optional future ACP adapters
pi-app          CLI/TUI/bootstrap, concrete providers and built-in tools
```

Зависимости должны быть направлены только внутрь:

```text
pi-session -> pi-core
pi-provider -> pi-core
pi-tools -> pi-core
pi-resources -> pi-core

pi-agent -> pi-core + pi-session + pi-provider + pi-tools
pi-protocol -> pi-core + pi-agent
pi-app -> all crates + concrete adapters
```

Стрелка означает «зависит от»; foundation crates не зависят от CLI, TUI, `$HOME`, environment или terminal state.

Foundation crates не читают process environment, `$HOME`, stdin или terminal state. Эти политики принадлежат `pi-app`.

## Workstreams

Работы делятся на самостоятельные workstreams, но интегрируются только в serial milestones ниже.

| Workstream | Область | Главный handoff |
| --- | --- | --- |
| WS-A Product/compatibility | upstream pin, manifests, claims, decisions | machine-readable compatibility manifest |
| WS-B Core/runtime | canonical types, errors, cancellation, agent loop | bounded runtime facade and event stream |
| WS-C Persistence/resources | JSONL, migrations, atomic writes, settings, trust, skills | fallible repositories and resolved runtime context |
| WS-D Providers/auth | provider contract, streams, transport, credentials | normalized terminal assistant message |
| WS-E Tools/safety | schemas, path policy, bash lifecycle, limits | cancellable tool executor |
| WS-F Protocol/product | print, JSON, RPC, optional ACP, TUI | one runtime exposed through multiple frontends |
| WS-G Extensions | discovery, trust and optional ABI | versioned registration/execution boundary |
| WS-H Delivery | CI, fixtures, target matrix, release | validated immutable artifacts |

Ни один workstream не интегрируется напрямую с ещё нестабильными внутренними структурами соседнего workstream. Handoff происходит только через перечисленные exit-gate artifacts.

## Tasks

1. **Milestone 1 — Product contract и безопасный hermetic vertical slice**
   - **User scenario:** разработчик запускает новый binary в пустом temporary HOME и workspace с deterministic fixture provider: prompt проходит через CLI, agent loop и read-only tool, результат возвращается в text и JSON modes; сеть, shell, запись файлов, project resources и реальные credentials не используются.
   - **Files/modules likely changed:**
     - `Cargo.toml`
     - `src/main.rs`
     - `src/lib.rs`
     - `crates/pi-core/Cargo.toml`
     - `crates/pi-core/src/{lib,message,model,tool,error,event}.rs`
     - `crates/pi-provider/src/{lib,provider}.rs`
     - `crates/pi-tools/src/{lib,registry}.rs`
     - `crates/pi-session/src/{lib,memory}.rs`
     - `crates/pi-agent/src/{lib,runtime}.rs`
     - `crates/pi-app/src/{lib,cli,bootstrap}.rs`
     - `docs/compatibility/upstream-policy.md`
     - `docs/compatibility/upstream-manifest.json`
     - `docs/architecture/runtime-boundary.md`
     - `docs/security.md`
   - **Changes:**
     - Зафиксировать upstream commit `f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee`, package version `0.83.0`, evidence paths и SHA256 fixtures в manifest.
     - Добавить workspace и новые crates; новый binary временно собирать как `pi-next`, не удаляя старый `pi`.
     - Определить единственные canonical `Message`, content blocks, `ToolCall`, `ToolResult`, `Usage`, `StopReason`, `Model`, `PiError`, `AgentEvent`.
     - Реализовать минимальный bounded agent loop: один provider request, максимум один разрешённый read-only tool round, затем terminal response.
     - Добавить test-only fixture provider/transport и read-only fixture tool, работающий только с явно переданным test root. Fixture provider не является production provider ID: он доступен только test-support harness.
     - Реализовать text и NDJSON renderers с жёстким разделением stdout/stderr.
     - Запретить в vertical slice environment discovery, network clients, shell, filesystem mutations, automatic resource discovery и extensions.
     - Ввести terminal invariant: каждый turn заканчивается ровно одним `Completed`, `Failed`, `Cancelled` или `LimitExceeded`.
     - Зафиксировать security contract: tools в будущем будут работать с правами invoking user; project trust не является sandbox.
   - **Dependencies:** нет кодовых зависимостей от последующих milestones; требуется принятие upstream pin и начальных ADR.
   - **Tests:**
     - `crates/pi-app/tests/vertical_slice_test.rs` — binary subprocess с temp HOME/workspace и test-support bootstrap.
     - `crates/pi-core/tests/core_wire_test.rs` — canonical message/event serialization.
     - `crates/pi-agent/tests/agent_limits_test.rs` — repeated tool request останавливается limit guard.
     - `crates/pi-app/tests/stdout_purity_test.rs` — каждая JSON-mode строка парсится как JSON.
     - Hermetic test harness запрещает outbound network не только архитектурным отсутствием adapters, но и через deny-network listener/firewall/container policy, где это доступно; отдельно проверяется отсутствие чтения реального HOME.
   - **Acceptance / exit gate:**
     - Test-support harness запускает fixture transport и выдаёт deterministic final text; production binary не принимает `--provider fixture`.
     - JSON mode выдаёт только валидный NDJSON; diagnostics находятся только в stderr.
     - Fixture tool не может писать, запускать процесс или читать вне переданного fixture root; acceptance включает absolute paths, `..`, symlinks, symlink swap/race policy и Unicode paths.
     - Turn всегда возвращается в `Idle`, включая provider error и limit exceeded.
     - `cargo test -p pi-app --test vertical_slice_test --features test-support` проходит offline; agent/core tests запускаются отдельными package targets.
     - Handoff seam: опубликованы `Provider`, `SessionRepository`, `ToolExecutor`, `AgentRuntime` и `AgentEvent` без зависимости от CLI/TUI.
   - **Non-goals:**
     - Session file persistence и upstream JSONL parity.
     - Реальные HTTP providers и API keys.
     - Bash/write/edit, TUI, compaction, RPC, ACP и extensions.
     - Полная переработка старого `src/`.
   - **Risks:**
     - Fixture provider может превратиться в альтернативную production-логику; он должен оставаться `test-support`.
     - Слишком широкий initial API будет трудно менять; публичными оставлять только handoff traits и canonical wire types.
     - Нельзя объявлять vertical slice полноценным продуктом: это первая исполняемая, безопасная foundation gate.

2. **Milestone 2 — Crash-safe sessions, settings и project trust**
   - **User scenario:** пользователь создаёт сессию, перезапускает `pi-next`, продолжает тот же branch, а corrupt или unwritable файл даёт понятную ошибку без потери предыдущей валидной версии; непроверенный project не влияет на settings или prompts.
   - **Files/modules likely changed:**
     - `crates/pi-session/src/{entry,file_repository,migration,tree,context,validation}.rs`
     - `crates/pi-resources/src/{settings,trust,loader,paths}.rs`
     - `crates/pi-core/src/message.rs`
     - `crates/pi-agent/src/runtime.rs`
     - `crates/pi-app/src/bootstrap.rs`
     - `tests/fixtures/upstream/sessions/**`
     - `tests/fixtures/upstream/settings/**`
   - **Changes:**
     - Реализовать discriminator-driven upstream session entry schema без потери полей: `parentSession`; все AgentMessage/entry варианты, включая `BashExecutionMessage` и `session_info`; labels включая clear; `CustomMessage.details/display`; compaction/branch `usage/details/fromHook`; assistant `api/errorMessage`; tool-result `details/usage/images/isError`; omitted-vs-null semantics и сохранение неизвестных `details` payload.
     - Реализовать v1→v2→v3 migrations и запретить объявлять `CURRENT_SESSION_VERSION=3` до прохождения fixtures.
     - Разделить pure context projection и IO repository.
     - Проверять header, version, duplicate IDs, parent links, cycles и line parse errors; recovery сделать отдельной явной операцией с backup.
     - Реализовать atomic same-directory rewrite, checked append, flush/sync policy и writer conflict detection.
     - Создавать user-state directories с `0700`, session/global settings files с `0600` на Unix.
     - Подключить `--session`, `--session-dir`, `--continue`, `--resume`, `--no-session` с различимой семантикой.
     - Реализовать global `~/.pi/agent/settings.json`, project `.pi/settings.json` и `~/.pi/agent/trust.json`.
     - Ввести двухфазный flow `PreTrustInventory -> TrustDecision -> ResolvedResources`. Trust должен покрывать `.pi/settings.json`, `.pi/**`, themes, prompts, skills включая ancestor `.agents/skills`, AGENTS/CLAUDE/SYSTEM/APPEND_SYSTEM, package installation/dependencies, extension manifests и execution. Pinned upstream pre-trust hooks явно зафиксировать как supported или `Unsupported`; нельзя читать/исполнять project-controlled resources до решения.
     - В headless mode значение `ask` без explicit override должно означать deny, не чтение stdin.
   - **Dependencies:** canonical types и runtime traits Milestone 1.
   - **Tests:**
     - `tests/session_wire_parity_test.rs`
     - `tests/session_migration_test.rs`
     - `tests/session_context_test.rs`
     - `tests/session_corruption_test.rs`
     - `tests/session_durability_test.rs`
     - `tests/session_paths_test.rs`
     - `tests/project_trust_test.rs`
     - Unix permissions и atomic-write fault injection.
   - **Acceptance / exit gate:**
     - Upstream v1/v2/v3 fixtures загружаются, мигрируются и round-trip без потери contract fields из schema checklist; valid-document wire compatibility отделена от strict-reader behavior.
     - Rust-created v3 fixture читается pinned upstream parser либо локальным offline oracle.
     - Malformed middle line, invalid header, dangling parent и duplicate ID не пропускаются молча.
     - Ошибка rewrite оставляет старый файл byte-for-byte неизменным.
     - Untrusted project marker не попадает в effective settings/system context.
     - Handoff seam: `OpenedSession` и `ResolvedResources` являются fallible inputs для runtime; runtime не знает путей `$HOME`.
   - **Non-goals:**
     - Автоматическое «исправление» corrupt sessions.
     - Multi-user storage или remote session authorization.
     - LLM compaction и branch summarization.
     - Загрузка executable extensions.
   - **Risks:**
     - Upstream parser может пропускать отдельные malformed lines; любое намеренно более строгое поведение нужно отметить как `Rust extension`, а не назвать parity.
     - Atomic rename/fsync и file locking различаются по платформам; до M2 ADR фиксирует single-writer или lock/CAS mechanism, stale-lock policy и platform durability matrix.
     - Legacy Rust sessions могут не совпадать ни с upstream, ни с новой схемой; нужен отдельный importer, а не неявная deserialization.

3. **Milestone 3 — Narrow real-provider slice, auth и streaming safety**
   - **User scenario:** пользователь с API key выполняет text/tool prompt через один полностью аттестованный API family; тот же код проходит hermetic mock-HTTP tests для streaming, timeout, cancellation, usage и errors.
   - **Files/modules likely changed:**
     - `crates/pi-provider/src/{request,stream,registry,error,transport}.rs`
     - `crates/pi-app/src/providers/{openai_compatible,anthropic}.rs`
     - `crates/pi-app/src/auth/{storage,resolver}.rs`
     - `crates/pi-agent/src/runtime.rs`
     - `crates/pi-session/src/entry.rs`
     - `tests/fixtures/upstream/providers/**`
     - `tests/fixtures/upstream/streams/**`
   - **Changes:**
     - Зафиксировать provider-neutral `ChatRequest` и stream lifecycle: `Started`, typed deltas, ровно один terminal `Completed|Failed|Cancelled`.
     - Протянуть cancellation token и deadline через agent, provider и HTTP body streaming.
     - Добавить connect/request/body timeout, bounded error body и redaction prompts/tokens/API keys.
     - Ввести retry state machine: transient classification, max attempts, exponential backoff, capped `Retry-After`, cancellation during backoff и запрет retry после видимых stream deltas, если операция не доказана безопасной. RPC retry controls остаются unsupported до отдельного protocol gate.
     - Реализовать ровно одну API family, выбранную отдельной ADR по pinned upstream auth/transport oracle. Вторая family и custom base URLs остаются deferred/partial до отдельного решения; конкретные provider IDs допускаются только через catalog data.
     - Нормализовать tool-call arguments/IDs, thinking, images, usage, cost и stop reasons на adapter boundary.
     - Реализовать auth resolution с точным provider→environment mapping, runtime `--api-key`, atomic `0600` storage и ясной precedence.
     - Persist terminal assistant message без реконструкции и потери usage/thinking/tool metadata.
   - **Dependencies:** sessions и resolved settings Milestone 2.
   - **Tests:**
     - `tests/provider_contract_test.rs`
     - `tests/provider_stream_test.rs`
     - `tests/provider_http_golden_test.rs`
     - `tests/provider_timeout_test.rs`
     - `tests/auth_resolution_test.rs`
     - Hanging server, slow body, malformed SSE/JSON, interleaved tool deltas, retry-after и cancellation race.
   - **Acceptance / exit gate:**
     - Обе заявленные API families проходят один общий offline contract suite.
     - Hanging request завершается по deadline; user cancellation быстрее общего timeout.
     - Ошибка не содержит request body, bearer token или marker prompt.
     - Terminal response сохраняется в session с provider/model/usage/stop reason.
     - Live smoke является optional/manual и не входит в hermetic gate.
     - Handoff seam: provider возвращает только normalized core events/messages; wire DTO приватны adapters.
   - **Non-goals:**
     - Broad provider catalog и OAuth provider login.
     - Bedrock, Codex, Copilot, Vertex, gateways и все OpenAI-compatible бренды.
     - Live provider tests в PR CI.
   - **Risks:**
     - Один общий transport не устраняет provider quirks; adapters должны иметь отдельные golden fixtures.
     - Provider IDs и model IDs могут конфликтовать; registry обязан возвращать typed conflict вместо nondeterministic winner.
     - Retry может удвоить небезопасный request; политика retry должна учитывать момент начала streamed response.

4. **Milestone 4 — Production tools, bounded turns и token-safe compaction**
   - **User scenario:** пользователь просит прочитать, изменить файл и выполнить bounded shell command; timeout/cancellation останавливает process tree, а длинная сессия compact-ится без orphan tool results.
   - **Files/modules likely changed:**
     - `crates/pi-tools/src/{definition,registry,context,result,path_policy}.rs`
     - `crates/pi-app/src/tools/{read,write,edit,grep,find,ls,bash}.rs`
     - `crates/pi-agent/src/{runtime,limits}.rs`
     - `crates/pi-session/src/{compaction,context}.rs`
     - `crates/pi-app/src/compaction/{cut_point,summary}.rs`
   - **Changes:**
     - Реализовать async tool API с cancellation, deadline, typed errors и validated JSON Schema.
     - Централизовать path resolution, но не выдавать его за containment: tools работают с OS-правами пользователя.
     - Добавить configurable и hard maximum для tool rounds/calls.
     - Реализовать bash через `tokio::process`, bounded stdout/stderr, process-group termination и reap.
     - Различать spawn error, non-zero exit, timeout и cancellation.
     - Реализовать token-aware compaction trigger, safe cut point, preservation tool-call/result pairs и persisted compaction entry.
     - LLM summary выполнять через provider contract; provider failure не должен частично фиксировать compaction.
     - Добавить branch summary как отдельную operation над session tree.
   - **Dependencies:** provider streaming/cancellation Milestone 3 и durable sessions Milestone 2.
   - **Tests:**
     - `tests/tool_schema_test.rs`
     - `tests/filesystem_policy_test.rs`
     - `tests/bash_timeout_test.rs`
     - `tests/tool_loop_limits_test.rs`
     - `tests/compaction_cutpoint_test.rs`
     - `tests/compaction_persistence_test.rs`
     - `tests/branch_summary_test.rs`
   - **Acceptance / exit gate:**
     - `sleep 2` при `timeoutMs=50` завершается bounded timeout и не оставляет descendant на аттестованной Unix platform.
     - Infinite-output process не превышает configured memory/output bounds.
     - Repeated malicious tool calls останавливаются hard limit.
     - Ни один retained context не начинается orphan tool result.
     - Reload после compaction восстанавливает эквивалентный active context.
     - Handoff seam: runtime предоставляет стабильные prompt/cancel/compact/tree operations, не раскрывая storage internals.
   - **Non-goals:**
     - Откат уже выполненных shell/filesystem side effects.
     - In-process filesystem sandbox.
     - Parallel tool execution.
     - Полное совпадение provider tokenizer; используется зафиксированная compatibility heuristic.
   - **Risks:**
     - Process-tree cleanup платформозависим; Windows нельзя считать поддержанным без Job Objects.
     - File mutation после cancellation может быть частично завершена; это должно быть отражено в UI и документации.
     - Compaction fixture divergence требует решения в compatibility manifest, а не настройки нормализатора до «зелёного» результата.

5. **Milestone 5 — Headless product surface и upstream RPC**
   - **User scenario:** automation запускает `pi --mode rpc`, отправляет LF-delimited commands, получает correlated responses/events и может prompt, abort, inspect state и создать новую сессию без stdout contamination.
   - **Files/modules likely changed:**
     - `crates/pi-protocol/src/{json_event,jsonl,rpc_types,rpc_server}.rs`
     - `crates/pi-app/src/{cli,headless}.rs`
     - `crates/pi-app/src/bin/pi-next.rs`
     - `tests/rpc_test.rs`
     - `tests/fixtures/upstream/rpc/**`
     - `docs/protocol/rpc.md`
   - **Changes:**
     - Реализовать typed modes `text`, `json`, `rpc`; неизвестные mode отклонять Clap parser.
     - Text mode печатает только final assistant text; JSON mode — ordered NDJSON events.
     - Реализовать upstream command/event JSONL, а не JSON-RPC 2.0.
     - Первая обязательная matrix: `prompt`, `steer`, `follow_up`, `abort`, `get_state`, `get_messages`, `get_entries`, `get_tree`, `new_session`, model/thinking setters, queue modes, compact и command listing.
     - Сохранить optional string correlation ID без преобразований.
     - Обрабатывать CRLF, fragmented input, final line without LF, malformed JSON и unknown command без падения процесса.
     - Гарантировать один prompt acceptance response; дальнейший failure приходит lifecycle event.
     - Один runtime должен обслуживать print, JSON и RPC, сохраняя одинаковые session/tool/provider side effects.
   - **Dependencies:** стабильный runtime handoff Milestone 4.
   - **Tests:**
     - `tests/headless_modes_test.rs`
     - `tests/rpc_jsonl_test.rs`
     - `tests/rpc_protocol_test.rs`
     - `tests/rpc_queue_abort_test.rs`
     - `tests/rpc_stdout_purity_test.rs`
     - Binary subprocess harness с fake provider и temp HOME.
   - **Acceptance / exit gate:**
     - Каждая stdout line RPC/JSON парсится как JSON.
     - Process остаётся жив после malformed/unknown command.
     - Steering/follow-up/abort ordering проходит deterministic tests.
     - EOF и signal завершают pending turn и освобождают children/files.
     - Generic JSON-RPC stand-in test удалён или полностью заменён.
     - Handoff seam: `pi-protocol` зависит только от public runtime facade.
   - **Non-goals:**
     - ACP support.
     - Extension-specific UI commands.
     - HTML export/share и весь upstream command set за пределами manifest.
     - TUI rendering.
   - **Risks:**
     - Слишком раннее закрепление неполного RPC может создать долгосрочный wire contract; protocol version и capability listing обязательны.
     - Event ordering при backpressure/cancellation требует bounded channels и единого writer task.
     - Human logging из concrete adapters может загрязнить stdout; protocol writer должен единолично владеть stdout.

6. **Milestone 6 — ACP decision gate**
   - **User scenario:** embedding/client maintainer получает однозначный ответ, поддерживает ли релиз ACP, какую именно версию и как ACP соотносится с upstream RPC; отсутствие решения не маскируется внутренним adapter trait.
   - **Files/modules likely changed:**
     - `docs/adr/0002-acp-support.md`
     - `docs/compatibility/upstream-policy.md`
     - при принятии: `crates/pi-protocol/src/acp/**`
     - при отклонении: только docs/capability declarations.
   - **Changes:**
     - До реализации зафиксировать внешний ACP spec repository/version/commit, transport, lifecycle ownership, authentication, session mapping и cancellation semantics.
     - Сравнить варианты:
       1. не поддерживать ACP в первом релизе;
       2. отдельный `pi-acp` adapter process поверх `AgentRuntime`;
       3. встроенный transport в `pi-protocol`.
     - Рекомендуемый default: **defer ACP из первого compatibility release**, пока нет pinned specification и независимых conformance fixtures.
     - Если ACP принимается, реализовать отдельный adapter; не изменять upstream RPC wire contract и не смешивать namespaces/events.
   - **Dependencies:** стабильный runtime и RPC separation Milestone 5.
   - **Tests:**
     - При defer: capability/help tests подтверждают отсутствие ложного ACP claim.
     - При принятии: official/pinned conformance fixtures, transport framing, lifecycle, cancellation и session mapping tests.
   - **Acceptance / exit gate:**
     - ADR имеет статус Accepted и содержит pinned ACP specification либо явное `Deferred`.
     - README, `--help` и capability APIs согласованы с решением.
     - Handoff seam: ACP, если принят, использует только `AgentRuntime`; core не зависит от ACP.
   - **Non-goals:**
     - Самостоятельное изобретение «ACP-like» протокола.
     - Объявление ACP через наличие generic JSON-RPC.
     - Преобразование upstream RPC в ACP путём переименования полей.
   - **Risks:**
     - Термин ACP может относиться к меняющемуся внешнему стандарту; без pinned source совместимость не проверяема.
     - Одновременное закрепление двух embedding protocols увеличит support burden.

7. **Milestone 7 — TUI и resource compatibility**
   - **User scenario:** пользователь запускает `pi-next` без headless flags, вводит Unicode/multiline prompt, видит streaming/tool states, отменяет turn и выходит без оставшегося raw mode; trusted project может подключить upstream-compatible skill и prompt.
   - **Files/modules likely changed:**
     - `crates/pi-resources/src/{skills,prompts,system_prompt}.rs`
     - `crates/pi-app/src/tui/{app,state,render,terminal,events,input}.rs`
     - `crates/pi-app/src/theme/{schema,loader}.rs`
     - `tests/fixtures/upstream/skills/**`
     - `tests/fixtures/themes/**`
   - **Changes:**
     - Реализовать recursive `SKILL.md` discovery, YAML frontmatter, validation, source precedence и `/skill:name`.
     - Реализовать prompt templates, argument substitution, AGENTS/CLAUDE/SYSTEM/APPEND_SYSTEM resolution через trust-aware loader.
     - Передавать system prompt отдельно от persisted upstream messages.
     - Создать pure TUI reducer и RAII terminal guard.
     - Multiplex terminal events, agent events, ticks и shutdown асинхронно.
     - Использовать grapheme-aware cursor model и display width.
     - Реализовать Theme v1 как документированное mapping, не заявляя full upstream theme schema parity.
   - **Dependencies:** headless runtime Milestone 5 и trust/resources foundation Milestone 2.
   - **Tests:**
     - `tests/skills_parity_test.rs`
     - `tests/prompt_templates_test.rs`
     - `tests/system_prompt_test.rs`
     - `tests/tui_state_test.rs`
     - `tests/tui_render_test.rs`
     - `tests/pty_tui_test.rs`
     - Unicode cases: Cyrillic, CJK, combining marks, emoji/ZWJ.
   - **Acceptance / exit gate:**
     - Upstream `SKILL.md` fixtures дают ожидаемые names/diagnostics/collision winner.
     - Untrusted project resources не влияют на prompt.
     - TUI вызывает реальный `AgentRuntime`, а не placeholder response.
     - PTY tests подтверждают восстановление canonical/echo/cursor/alternate-screen state после success, error, Ctrl-C и EOF.
     - Handoff seam: TUI — consumer событий runtime; она не владеет provider/tool/session semantics.
   - **Non-goals:**
     - Полное визуальное совпадение с upstream.
     - Images/pixel protocols, external editor и все upstream widgets.
     - Extension-provided custom TUI components.
     - User-configurable keybinding DSL в первом релизе.
   - **Risks:**
     - PTY tests могут быть flaky; использовать readiness handshake и bounded waits, не sleeps.
     - Unicode cursor correctness требует единой coordinate system во всех editor actions.
     - Resource precedence нельзя определять «интуитивно»; она должна следовать imported fixtures/manifest.

8. **Milestone 8 — Extension ABI decision и минимальная реализация**
   - **User scenario:** пользователь знает, совместимы ли его upstream TypeScript extensions; если включён Rust ABI, executable extension запускается только после trust decision, а crash/timeout не завершает `pi`.
   - **Files/modules likely changed:**
     - `docs/adr/0003-extension-abi.md`
     - `docs/extensions.md`
     - `crates/pi-app/src/extensions/{manifest,discovery,registry,process}.rs`
     - `crates/pi-protocol/src/extension_v1.rs`
     - `tests/fixtures/extensions/**`
   - **Changes:**
     - Отдельно решить один из вариантов: embedded JS/jiti compatibility host, subprocess JSONL ABI, native dynamic libraries или отсутствие executable extensions.
     - Рекомендуемый вариант первого Rust release: **subprocess JSONL ABI v1**, явно `Mapped/Rust extension`, не wire/runtime compatible с upstream TypeScript extensions.
     - Зафиксировать manifest version, executable/args, capabilities, permissions, supported modes, handshake и shutdown.
     - Требовать explicit trust до запуска executable.
     - Ввести bounded line/message size, startup/request timeout, stderr redaction и child cleanup.
     - Минимальный ABI: command registration, tool descriptor/execution, lifecycle notification и diagnostics.
     - Сохранить upstream TypeScript compatibility как отдельный будущий adapter-host project, не скрывать её отсутствие.
   - **Dependencies:** RPC framing/process conventions Milestone 5 и project trust Milestone 2.
   - **Tests:**
     - `tests/extension_discovery_test.rs`
     - `tests/extension_process_test.rs`
     - Wrong ABI, malformed output, oversized line, timeout, crash, untrusted executable и cleanup cases.
   - **Acceptance / exit gate:**
     - ADR принят до появления production execution code.
     - Extension нельзя запустить без trust decision.
     - Crash/timeout локализуется и даёт structured diagnostic.
     - JSON/RPC stdout не загрязняется extension stderr.
     - Compatibility table прямо говорит, что upstream `.ts` extensions не запускаются.
     - Handoff seam: extension registry использует публичные command/tool registration APIs, не mutable runtime internals.
   - **Non-goals:**
     - Загрузка `.so`, `.dylib` или `.dll` в процесс.
     - Rust compiler ABI compatibility.
     - Embedded Node/jiti в первом релизе.
     - Полный upstream extension UI API.
   - **Risks:**
     - Subprocess separation не является полноценным sandbox: child имеет OS-права пользователя.
     - Tool registration может расширить attack surface; permissions и user-visible capabilities обязательны.
     - Неполный ABI нельзя бездумно расширять breaking fields; нужен version negotiation.

9. **Milestone 9 — Sandbox decision gate**
   - **User scenario:** пользователь однозначно понимает, защищает ли `pi` host; product не запускает `sudo`, mount scripts или project-controlled sandbox config под видом надёжной изоляции.
   - **Files/modules likely changed:**
     - `docs/adr/0004-sandbox-policy.md`
     - `docs/security.md`
     - `README.md`
     - `README_EN.md`
     - `src/sandbox/{mod,config,epkg}.rs`
     - `src/cli/args.rs`
     - `src/main.rs`
     - legacy `docs/plans/2026-03-05-sandbox-feature.md`
   - **Changes:**
     - Отдельно принять sandbox policy; рекомендуемое решение: **не включать текущий built-in epkg launcher в поддерживаемый продукт**.
     - Удалить из нового binary `--sandbox`, automatic `.pi/sandbox.json`, `sudo`, generated root shell, ignored mount/pivot failures и broad credential propagation.
     - Старый код удалить при final cutover либо изолировать за `experimental-unsafe-sandbox`, который не собирается в release artifacts.
     - Документировать external container/VM/micro-VM deployment с explicit mounts/network/credentials.
     - Не ограничивать обычные tools скрытым workspace prefix check: без OS boundary это не sandbox.
   - **Dependencies:** security contract Milestone 1; не блокирует core work, но блокирует public release.
   - **Tests:**
     - CLI help snapshot не содержит production sandbox flags.
     - Source policy test не находит `sudo`, `pivot_root` и shared `/tmp/sandbox-pi-rs` в release modules.
     - Malicious `.pi/sandbox.json` игнорируется.
     - Optional external-container smoke проверяет deployment, но не называется встроенной sandbox attestation.
   - **Acceptance / exit gate:**
     - ADR принят и отражён в docs/help.
     - Release binary не выполняет legacy launcher.
     - Product нигде не обещает host protection.
     - Handoff seam: external sandbox оборачивает весь process и не влияет на core ABI.
   - **Non-goals:**
     - Косметическое укрепление текущего generated shell.
     - Частичный in-process filesystem sandbox.
     - Защита от prompt injection.
     - Обещание, что cancellation откатывает side effects.
   - **Risks:**
     - Удаление advertised feature может потребовать migration note.
     - Возврат встроенного sandbox в будущем потребует отдельного threat model, platform matrix и fail-closed adversarial suite.

10. **Milestone 10 — Broad provider parity decision и controlled expansion**
    - **User scenario:** пользователь видит точный список аттестованных providers/API families и не принимает наличие model ID в каталоге за доказательство поддержки.
    - **Files/modules likely changed:**
      - `docs/adr/0005-provider-matrix.md`
      - `docs/compatibility/provider-matrix.json`
      - `crates/pi-app/src/providers/**`
      - `crates/pi-app/src/auth/**`
      - `tests/fixtures/upstream/providers/**`
      - `tests/provider_contract_test.rs`
    - **Changes:**
      - Отдельно утвердить target matrix по API families, а не по маркетинговым provider names.
      - Рекомендуемый порядок расширения:
        1. OpenAI Responses/Codex;
        2. Google Generative AI и Vertex;
        3. Mistral Conversations;
        4. Bedrock Converse;
        5. OAuth/Copilot providers;
        6. gateways/OpenAI-compatible catalog entries.
      - Для каждой family добавить model metadata, auth/OAuth, request/stream normalization, retry и fixture coverage.
      - Catalog listing отделить от credential availability.
      - Не регистрировать Ollama как неявный default; пометить его как Rust/custom provider.
      - Provider получает статус supported только после всех обязательных capabilities или с явно перечисленным partial profile.
    - **Dependencies:** narrow provider foundation Milestone 3; не блокирует TUI/RPC для уже поддержанных providers.
    - **Tests:**
      - Общий provider contract suite запускается для каждой заявленной family.
      - Provider-specific HTTP goldens.
      - OAuth refresh/expiry and auth precedence.
      - Tool calls, thinking, images, usage/cost, retry-after, cancellation and malformed stream.
    - **Acceptance / exit gate:**
      - `provider-matrix.json` генерирует docs/help listing.
      - Ни одна supported строка не существует без contract test.
      - Catalog работает offline.
      - Полная upstream provider parity заявляется только при отсутствии unexplained gaps относительно pinned commit.
      - Handoff seam: новые adapters не требуют изменений `pi-agent`.
    - **Non-goals:**
      - Одновременная реализация всех providers.
      - Live network tests как compatibility oracle.
      - Копирование почти одинаковых clients вместо API-family adapters.
      - Скрытое fallback-перенаправление на другой provider.
    - **Risks:**
      - OAuth flows зависят от внешних services и требуют replayable fixtures.
      - Upstream catalog может часто меняться; pinned manifest защищает текущий release, но создаёт update workload.
      - Partial capability profiles должны быть видимы пользователю до выбора модели.

11. **Milestone 11 — Cutover, migration и release candidate**
    - **User scenario:** существующий пользователь устанавливает новый `pi`, получает backup/diagnostics для legacy state, может безопасно откатиться и скачивает checksum-verified artifact.
    - **Files/modules likely changed:**
      - `Cargo.toml`
      - `src/main.rs`
      - `src/lib.rs`
      - `README.md`
      - `README_EN.md`
      - `CHANGELOG.md`
      - `docs/migration.md`
      - `docs/releasing.md`
      - `.github/workflows/{ci,security,release}.yml`
      - `release/targets.toml`
      - `scripts/{package-release,verify-release-assets,smoke-release}.sh`
    - **Changes:**
      - После всех обязательных gates переключить `pi` на `pi-app`; удалить `pi-next`.
      - Удалить binary-local duplicate `mod` universe и legacy placeholder frontend.
      - Оставить read-only legacy import command: scan → report → backup → explicit migrate.
      - Не перезаписывать legacy auth/settings/session files без successful parse, backup и user confirmation, где преобразование lossy.
      - Обновить README claims строго из compatibility matrices.
      - Собрать target matrix только после native smoke validation.
      - Release workflow: explicit tag/source ref, immutable source archive, build artifacts, SHA256SUMS, separate validation job, draft release, protected approval, publish without rebuild.
    - **Dependencies:** все release-blocking milestones и decision gates.
    - **Tests:**
      - Legacy import fixtures и rollback.
      - `pi --version`, `--help`, text/JSON/RPC no-network smoke из распакованного archive.
      - Linux PTY startup/quit.
      - Corrupted checksum, extra/missing asset и version mismatch negative tests.
    - **Acceptance / exit gate:**
      - Новый `pi` использует один library/workspace type universe.
      - Legacy state остаётся неизменным при failed/dry-run migration.
      - Exact asset allowlist и checksums проверены до draft creation.
      - Публикуется тот же artifact, который был validated.
      - Public release не содержит experimental sandbox и не заявляет deferred ACP/upstream TS extension compatibility.
      - Handoff seam: release artifact связан с source SHA, upstream manifest SHA и compatibility matrix version.
    - **Non-goals:**
      - In-place silent migration.
      - Mutation уже опубликованного release.
      - Package-manager repositories, npm publication, Homebrew/Scoop в первом release.
      - Bit-for-bit reproducibility claim без отдельной toolchain/linker attestation.
    - **Risks:**
      - Legacy Rust JSONL может быть неоднозначным; importer обязан предпочитать отказ потере данных.
      - Cross-platform TUI/process semantics могут сузить initial target matrix.
      - Release нельзя блокировать broad provider expansion, если narrow supported matrix честно документирована.

## CI и release plan

### CI stages

1. **Bootstrap CI с Milestone 1**
   - `cargo fmt --check`
   - `cargo check --workspace --all-targets --all-features`
   - Foundation and vertical-slice tests.
   - No-network enforcement для hermetic suites.

2. **Required PR gates после очистки baseline**
   - `cargo test --workspace --all-targets --all-features`
   - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   - Session fixtures/provenance.
   - Provider mock-HTTP contracts.
   - CLI/RPC subprocess tests.
   - Linux safety tests: permissions, atomic persistence, bash process cleanup.
   - Linux PTY lifecycle.

3. **Platform matrix**
   - Linux x86_64: all tests, PTY, process tree, permissions, release smoke.
   - Linux arm64: check/test and native release smoke before publication.
   - macOS x86_64/arm64: check/test; PTY required before claiming TUI support.
   - Windows x86_64: check/unit tests initially; process-tree and ConPTY required before full support.
   - Termux/Android: documented developer target only until repeatable runner exists.
   - Unsupported platform-specific behavior must fail compilation, fail closed or be explicitly excluded; silent semantic degradation запрещена.

4. **Supply chain**
   - Scheduled/manual `cargo audit`.
   - `cargo deny check advisories licenses bans sources`.
   - Pinned Rust toolchain and SHA-pinned GitHub Actions.
   - Default workflow `permissions: {}`; grant only per job.
   - No real provider credentials in CI.

### Release stages

1. Trigger by `v*` tag or recovery dispatch with explicit `source_ref`.
2. Verify tag version equals `Cargo.toml`.
3. Create source archive from exact ref.
4. Build declared targets from that archive.
5. Package binary, license, README and compatibility manifest.
6. Generate `SHA256SUMS`; optionally SBOM after tool-pinning decision.
7. Upload short-retention promotion artifact.
8. Separate validation job downloads artifacts and verifies exact allowlist, checksums, layout, executable bit and smoke tests.
9. Create draft GitHub Release.
10. Protected-environment approval publishes the validated artifacts without rebuild.
11. Failed pipeline may remove its draft but never mutate an already-public release.

## Test matrix

| Contract | Unit | Fixture/golden | Process/integration | Cross-implementation |
| --- | ---: | ---: | ---: | ---: |
| Core messages/events | yes | upstream JSON | runtime sequence | serde comparison |
| Sessions/migrations | yes | v1/v2/v3 JSONL | create/reload/corrupt/fault | upstream↔Rust reader |
| Settings/trust/resources | yes | project trees | temp HOME/headless | upstream expected resolution |
| Providers | yes | HTTP/stream transcripts | mock server/abort/timeout | normalized event comparison |
| Tools | yes | schema/output | filesystem/process tree | upstream semantics where specified |
| Compaction | yes | cut point/transcript | persist/reload | upstream fixture comparison |
| Text/JSON/RPC | yes | transcripts | spawned binary | pinned upstream subset |
| TUI/input/theme | reducer | snapshots/themes | Linux PTY | scenario comparison |
| Extensions | protocol | manifests/transcripts | child process | documented mapping only |
| ACP | conditional | official fixtures | adapter process | only if accepted |
| Release | script tests | asset allowlist | extracted binary smoke | source/artifact SHA linkage |

All integration suites run with isolated HOME, workspace, session directory and fake credentials. Tests must never inspect real `~/.pi`.

## Compatibility fixtures

Create `tests/fixtures/upstream/manifest.json` containing:

- Upstream repository SHA and package version.
- Original source path.
- Fixture SHA256.
- Relevant upstream test/source oracle.
- Allowed normalization fields.
- Expected compatibility level.

Fixture tree:

```text
tests/fixtures/upstream/
├── sessions/{v1,v2,v3,branched,compacted,corrupt}/
├── messages/
├── settings/
├── trust/
├── resources/
├── skills/
├── prompts/
├── providers/<api-family>/
├── streams/
├── compaction/
└── rpc/
```

Normalization may cover only generated timestamp, UUID and explicitly nondeterministic temp path. Она не должна удалять:

- `null` versus omitted distinctions;
- role/type casing;
- usage/cost/stop reason;
- thinking/tool-call content;
- parent IDs;
- JSONL ordering;
- protocol correlation IDs;
- unknown extension/details payloads.

Fixture changes require provenance check and compatibility review. Generated Rust fixtures хранятся отдельно в `tests/fixtures/rust/`.

## Migration strategy

1. **Parallel construction:** новый workspace и `pi-next` создаются рядом со старым binary; unsafe legacy sandbox сразу исключается из release/CI путей и доступен только за `experimental-unsafe-sandbox`.
2. **Read-only inventory:** `pi-next migrate inspect` перечисляет settings/auth/sessions/resources и возможные conflicts без записи.
3. **Session conversion:** upstream-valid sessions используются напрямую; Rust-legacy sessions преобразуются только через typed importer.
4. **Backup first:** перед любым rewrite создаётся timestamped backup в том же user-state root с manifest исходных hashes.
5. **Journaled per-artifact commit:** каждый файл записывается во временный sibling, sync-ится и заменяется атомарно после backup. Это не transaction-wide atomicity: journal/recovery report фиксируют частичный успех, порядок замен и rollback actions.
6. **No secret persistence from CLI:** runtime `--api-key` не записывается автоматически.
7. **Corruption policy:** corrupt files не «лечатся» автоматически; recovery — отдельная команда с отчётом о пропущенных данных.
8. **Rollback:** пока backup существует, пользователь может восстановить старое состояние; release notes описывают несовместимые поля.
9. **Cutover:** имя `pi` переключается только после прохождения обязательных gates; старый binary не остаётся скрытым fallback.
10. **Post-cutover cleanup:** legacy code удаляется после одного release-candidate cycle, но migration fixtures сохраняются постоянно.

## Decision log

| ID | Решение | Статус/рекомендация | Gate |
| --- | --- | --- | --- |
| D-001 | Upstream baseline | Accepted: commit `f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee`, coding-agent `0.83.0` | M1 |
| D-002 | Core architecture | Accepted: workspace с canonical types и injected adapters | M1 |
| D-003 | First vertical slice | Accepted: hermetic fixture provider + memory session + read-only fixture tool | M1 |
| D-004 | Session policy | Accepted direction: valid upstream v3 wire compatibility с explicit v1/v2 migrations; strict rejection/recovery — отдельная behavior policy | M2 |
| D-005 | Project trust | Accepted direction: input-loading guard, не sandbox; headless `ask` → deny | M2 |
| D-006 | Initial provider scope | Open: ровно одна API family, выбранная отдельной ADR по pinned upstream auth/transport oracle; broad matrix deferred | M3 |
| D-007 | Tool scheduling | Accepted direction: sequential, bounded и cancellable; authorization follows pinned upstream, otherwise explicit approval/headless policy | M4 |
| D-008 | RPC | Accepted direction: pinned upstream JSONL subset over local stdio only, не JSON-RPC 2.0; exact commands/fields/capabilities are manifest-owned | M5 |
| D-009 | ACP | Accepted: Deferred до pinned external spec/conformance fixtures | M6 |
| D-010 | Extension ABI | Accepted: executable extensions deferred; future subprocess JSONL is only a `Mapped/Rust extension`, not upstream TS compatibility | M8 |
| D-011 | Sandbox | Accepted: quarantine legacy launcher сразу за `experimental-unsafe-sandbox`; выключен в release/CI; external OS isolation only | M1/M9 |
| D-012 | Broad provider parity | Deferred to explicit family-by-family matrix after narrow slice | M10 |
| D-013 | Initial release targets | Open pending native CI/smoke evidence; не публиковать неподтверждённые targets | M11 |

Каждое решение со статусом Proposed/Open должно стать отдельным ADR со статусом Accepted или Deferred до соответствующего exit gate.

## CLI compatibility matrix

Перед M5 создать machine-readable matrix для каждого upstream CLI flag/argument: `supported`, `mapped` или `unsupported`, с precedence и black-box test. Обязательные строки первого release: repeated messages, `@file` и image input, `--tools`/`--exclude-tools`/`--no-tools`/`--no-builtin-tools`, `--session-id`/`--fork`/`--name`, `--models`/`--list-models`, system-prompt flags, trust overrides, `--offline`, export и UI mode. Неподдержанные строки должны давать явную диагностику или отсутствовать в help; accepted-but-ignored flags запрещены.

## RPC subset contract

`upstream-manifest.json` должен перечислять точные команды, поля и события M5: `prompt` (включая images и `streamingBehavior`), `steer`, `follow_up`, `abort`, `get_state`, `get_messages`, `get_entries`, `get_tree`, `new_session`, model/thinking setters, `get_commands`, `compact` и выбранные command listings. Остальные upstream commands помечаются `Unsupported`; unknown commands/fields получают определённую policy. Transport первого release — только local stdin/stdout; socket/network wrapper требует отдельной ADR. Generic JSON-RPC 2.0 не считается эквивалентом.

## Сознательные non-goals

В первом compatibility release сознательно не делать:

- Не копировать старый implementation plan или текущую структуру `src/` как целевую архитектуру.
- Не объявлять полную upstream parity одним флагом.
- Не пытаться реализовать все providers одновременно.
- Не считать наличие provider/model в registry доказательством поддержки.
- Не исполнять upstream TypeScript extensions без отдельного adapter host.
- Не загружать native dynamic plugins в процесс.
- Не поддерживать ACP без pinned specification.
- Не сохранять и не укреплять текущий `sudo`/mount generated-shell sandbox как production boundary.
- Не делать workspace-only path checks и не называть их sandbox.
- Не обещать rollback уже совершённых tool side effects.
- Не восстанавливать corrupt sessions молча.
- Не использовать live network/provider tests как PR gate.
- Не реализовывать parallel tools до стабилизации sequential lifecycle.
- Не обещать pixel-perfect upstream TUI, image protocols или все interactive widgets.
- Не заявлять full upstream theme schema parity; использовать документированное mapping.
- Не публиковать неподтверждённые targets.
- Не обещать bit-for-bit reproducible binaries без отдельной аттестации.
- Не добавлять package-manager distribution channels в первый release.

## Definition of Done

Реализация считается завершённой для первого compatibility release, только если:

1. Upstream manifest закрепляет точный SHA и fixture provenance.
2. Compatibility matrix не содержит необоснованных `wire-compatible`/`behavior-compatible` статусов.
3. Новый `pi` использует один canonical workspace type universe.
4. Hermetic vertical slice и все required suites проходят без сети и credentials.
5. Session v1/v2/v3 fixtures проходят migrations, round-trip и cross-reader checks.
6. Persistence не скрывает IO/serialization errors и проходит atomicity/permissions tests.
7. Project resources не загружаются до trust resolution.
8. Каждый заявленный provider проходит общий contract suite.
9. Provider и tool cancellation/timeout/limits имеют deterministic terminal outcome.
10. Bash process cleanup аттестован на каждой заявленной platform либо capability не заявлена.
11. Text, JSON и RPC используют один runtime; machine-readable stdout не загрязняется.
12. TUI вызывает настоящий runtime и восстанавливает terminal state во всех проверенных exit paths.
13. Extension, ACP и sandbox decisions отражены в ADR, help, README и compatibility matrix.
14. Legacy migration имеет inspect, backup, atomic commit и rollback path.
15. `cargo fmt`, workspace check/test и утверждённая clippy policy проходят.
16. Security/audit jobs проходят для release candidate.
17. Release artifacts построены из exact ref, проверены отдельным job и снабжены checksums.
18. README не рекламирует unsupported или experimental behavior.
19. Нет known blocker/high findings без явного release waiver, owner и срока.
20. Independent reviewer подтверждает manifest, test evidence, residual risks и release asset lineage.

## Files to Modify

- `Cargo.toml` — workspace, canonical dependencies и окончательный binary cutover.
- `src/main.rs` — сначала coexistence entrypoint, затем переключение на `pi-app`.
- `src/lib.rs` — временные compatibility re-exports и последующее удаление duplicate implementations.
- `src/cli/args.rs` — удаление misleading legacy flags при cutover.
- `src/sandbox/mod.rs` — quarantine/removal legacy sandbox.
- `src/sandbox/config.rs` — удаление automatic project sandbox input.
- `src/sandbox/epkg.rs` — удаление unsafe production launcher.
- `README.md` — точные claims, security и migration.
- `README_EN.md` — синхронные английские claims.
- `CHANGELOG.md` — migration/release notes.
- `Cargo.lock` — workspace и утверждённые зависимости.
- `tests/rpc_test.rs` — замена generic JSON-RPC stand-in upstream JSONL contract tests.
- Существующие `tests/*.rs` — перевод с local stand-ins/placeholder assertions на production APIs либо удаление как ложного покрытия.
- `docs/plans/2026-03-05-sandbox-feature.md` — пометка superseded/rejected после sandbox ADR.

## New Files

- `crates/pi-core/**` — canonical protocol/error/event types.
- `crates/pi-session/**` — memory/file repositories, schema, migrations, tree и context.
- `crates/pi-provider/**` — provider-neutral contracts and registry interfaces.
- `crates/pi-tools/**` — tool contracts, schemas, registry and execution context.
- `crates/pi-agent/**` — bounded cancellable runtime.
- `crates/pi-resources/**` — settings, trust, skills, prompts and system resources.
- `crates/pi-protocol/**` — JSON event, RPC и conditional ACP adapters.
- `crates/pi-app/**` — CLI/TUI/bootstrap, concrete providers and tools.
- `docs/compatibility/upstream-policy.md` — authoritative compatibility rules.
- `docs/compatibility/upstream-manifest.json` — pinned evidence and fixture hashes.
- `docs/compatibility/provider-matrix.json` — provider/API-family support.
- `docs/architecture/runtime-boundary.md` — dependency graph and invariants.
- `docs/security.md` — user permissions, trust and external isolation.
- `docs/migration.md` — inspect/backup/import/rollback.
- `docs/releasing.md` — release runbook.
- `docs/adr/0002-acp-support.md` — ACP decision.
- `docs/adr/0003-extension-abi.md` — extension ABI decision.
- `docs/adr/0004-sandbox-policy.md` — sandbox decision.
- `docs/adr/0005-provider-matrix.md` — broad provider decision.
- `tests/fixtures/upstream/**` — immutable imported/provenance-tracked fixtures.
- `tests/fixtures/rust/**` — Rust-generated fixtures.
- `tests/support/**` — fixture loader, fake provider, mock HTTP and process harness.
- `.github/workflows/ci.yml` — merge gates.
- `.github/workflows/security.yml` — audit/deny.
- `.github/workflows/release.yml` — staged release.
- `release/targets.toml` — authoritative target matrix.
- `scripts/package-release.sh` — deterministic packaging.
- `scripts/verify-release-assets.sh` — checksums/allowlist validation.
- `scripts/smoke-release.sh` — extracted artifact smoke tests.
- `rust-toolchain.toml` — pinned compiler/components.
- `deny.toml` — dependency policy.

## Dependencies

1. Milestone 1 создаёт все canonical handoff traits и блокирует последующую интеграцию; test ownership разделён по packages: `crates/pi-app/tests/vertical_slice_test.rs`, `crates/pi-agent/tests/agent_limits_test.rs`, `crates/pi-core/tests/core_wire_test.rs`.
2. Milestone 2 зависит от canonical messages/errors, но не от real providers.
3. Milestone 3 зависит от durable session and settings interfaces Milestone 2.
4. Milestone 4 зависит от cancellation/provider contracts Milestone 3 и persistence Milestone 2.
5. Milestone 5 зависит от stable runtime operations Milestone 4.
6. Milestone 6 зависит от protocol/core separation Milestone 5, но решение `Deferred` не блокирует дальнейшие milestones.
7. Milestone 7 зависит от runtime Milestone 5 и trust/resources Milestone 2.
8. Milestone 8 зависит от trust Milestone 2 и JSONL/process conventions Milestone 5.
9. Milestone 9 может готовиться параллельно, но является обязательным release gate.
10. Milestone 10 зависит от narrow provider contract Milestone 3 и может выполняться инкрементально после TUI/RPC.
11. Milestone 11 зависит от всех обязательных exit gates, принятых ADR и зелёного CI.

## Risks

- Запрошенный `/data/data/com.termux/files/home/context.md` отсутствовал во время планирования; roadmap основан на четырёх доступных audit reports, найденных scope plans, текущем `pi-rs` и pinned local `pi-mono`.
- Scope-plan paths из указанного run отсутствовали; были прочитаны одноимённые планы из доступного run `d4e1edd2-baa3-42fb-8d82-6b95efaa5409`. Аудит-отчёты из указанного run были доступны и прочитаны.
- Upstream commit соответствует package `0.83.0`, но package version не заменяет SHA pin.
- Переключение `main.rs` с duplicate module universe на library facade проявит latent type/API conflicts.
- Legacy session compatibility неизвестна до fixture inventory; нельзя менять serde schema без importer tests.
- Current sandbox содержит shell interpolation, `sudo` и fail-open mount behavior; его случайное сохранение в новом bootstrap является release blocker.
- Current session/auth/settings paths скрывают ошибки и могут терять данные; любое временное infallible compatibility API опасно.
- Platform promises ограничиваются доступностью native process/PTY/permissions tests.
- Extension subprocess ABI уменьшает blast radius crash, но не ограничивает filesystem/network права.
- Broad provider work может бесконечно задерживать релиз; narrow support matrix должна оставаться допустимым продуктовым результатом.
- Compatibility normalizers могут скрыть реальные расхождения; разрешённые normalization fields должны проходить review.
- CI baseline сейчас противоречив в аудитах (`cargo test --quiet` проходил, `cargo test --all-targets` не компилировался); Milestone 1 обязан установить один authoritative all-target baseline.
