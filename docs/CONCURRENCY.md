# Concurrency в OnyxKernel

Документ описывает модель синхронизации ядра после закрытия Приоритета 1 из
`todo.md` (все критические гонки/UAF/дыры безопасности, см. git-историю и
маркеры `todo P1 #N` в коде).

## Карта локов

| Лок | Защищает | Файл | Особенности |
|-----|----------|------|-------------|
| `G_PROC_LIST_LOCK` | `G_ALL_PROCS` (список всех Proc), публикация состояний, unlink/reap | `proc/process/globals.rs` | Внешний лок. try_lock-варианты в `dump_all`/`count()` — вызываются из panic-пути |
| `NET_LOCK` (рекурсивный) | `UDP_SOCKS`, `NEXT_UDP_PORT`, `CONNS`, `NEXT_PORT`, ARP-кэш, `IP_ID`, доступ к TX/RX-кольцам virtio-net | `net/lock.rs` | Рекурсивный per-hart: владелец может входить повторно |
| `G_QLOCK[dev]` | Очереди virtio (blk и др. через `virtio_req::request`) | `drivers/virtio/mod.rs` | Leaf-лок драйвера |
| Локи pmm / heap / vmm | Физические страницы, куча, таблицы страниц | `mm/` | Leaf-локи, берутся внутри NET_LOCK (аллокации в TX-пути) |
| rq-локи (`rq_lock(hart)`) | Per-hart runqueue | `proc/scheduler/runqueue.rs` | Вложенный внутрь `PROC_LIST_LOCK` (порядок см. ниже) |

Что НЕ требует лока:
- per-hart структуры: `G_PROCBUF_HARTS[hart]` (procfs, `fs/procfs/content.rs`),
  `G_HART_CURRENT[hart]` (пишет только владелец-харт);
- атомики: `G_NEXT_PID`, `G_USER_PAGES`, `G_NEED_RESCHED`, `BAD_CKSUM`;
- boot-only статики: `G_IP/G_GW/G_MASK` (пишутся один раз до старта
  вторичных хартов, дальше только читаются);
- конфигурация FDT-глобалов `G_DTB/G_STRUCT/...` — однопоточный boot-парс.

## Порядок вложенных локов

⚠️ Всегда брать локи в ОДНОМ порядке:

```text
PROC_LIST_LOCK  →  rq_lock(hart)      (exit.rs, spawn::publish_ready)
NET_LOCK        →  (heap/pmm)         (внутри virtio_net::send)
```

Хорошо:
```rust
let _ = proc_list_lock();          // внешний
rq_lock(caller_hart);              // вложенный
enqueue(caller_hart, p);
rq_unlock(caller_hart);
proc_list_unlock();
```

Плохо (deadlock): брать `PROC_LIST_LOCK`, уже держа `rq_lock`.

`NET_LOCK` — leaf: из-под него разрешены только heap/pmm/драйверные leaf-локи.
Никогда не брать `PROC_LIST_LOCK` под `NET_LOCK` и наоборот.

## Рекурсивный NET_LOCK: почему именно он

Граф вызовов сетевого стека вкладывает acquisitions на одном харте:

```text
udp_send / tcp_send / handle_icmp        (вход 1)
  └─ ip::send_packet                     (вход 2)
       └─ ARP miss: arp_request + poll   (вход 3)
            └─ eth::dispatch             (вход 4)
                 ├─ handle_arp → arp_insert
                 ├─ handle_udp → запись в кольцо UDP_SOCKS
                 └─ handle_tcp → мутация CONNS
```

Обычный SpinLock Self-дедлокнулся бы на втором входе; «отпустить лок вокруг
poll()» — значило бы снова открыть ту гонку, ради которой лок и введён.
`net/lock.rs` хранит `owner` (hart id) + `depth`: повторный вход владельца —
no-op, чужой харт крутится на нижележащем SpinLock.

Цена: сетевые операции сериализуются между хартами (в т.ч. long-poll
`tcp_connect`/ARP-резолв). Это сознательный компромисс — старый контракт
«single-poller/single-sender» требовал того же, просто не форсил его.

## Fork/waitpid: протокол публикации (P1 #1, #2)

1. `proc::create_user()` создаёт узел в состоянии `ProcState::Creating`
   и НЕ ставит его в runqueue — воркер-стилер физически не может взять
   полуинициализированного ребёнка.
2. Харт-родитель копирует всё наследуемое состояние (fds, signal_handlers,
   cwd, mmap_brk, tf) в `sys_fork`.
3. `proc::publish_ready(pid)` под `PROC_LIST_LOCK` атомарно переводит
   Creating → Ready и кладёт узел в runqueue (rq_lock внутри).
4. `sys_waitpid` публикует `Waiting` в ТОЙ ЖЕ критической секции, где
   проверял `has_child` (протокол B4, идентичный `proc::wait`) — окно
   lost-wakeup закрыто.

Boot-путь (`srv/main/init.rs → enter_user`) переводит Creating → Running
напрямую: вторичные харты ещё не запущены.

## Anti-UAF: sched_setaffinity (P1 #3)

`by_pid()` отпускает `PROC_LIST_LOCK` до возврата ссылки, поэтому запись
`p.affinity` была UAF-гонкой с конкурентным reap. Теперь lookup + проверка
`state != Exited` + запись affinity выполняются под `proc_list_lock`
(через `by_pid_unlocked`). Exited-процесс возвращает `ESRCH`.

## procfs: per-hart буфер (P1 #5)

Вместо общего `static mut G_PROCBUF` — `G_PROCBUF_HARTS[MAX_HARTS][1024]`,
индекс — `hart_id() % MAX_HARTS`. Слот трогает только владелец; SIE=0 в
kernel context гарантирует отсутствие same-hart прерываний внутри
`generate_content`. Хост-тесты используют слот 0.

## libfdt: bounds checking (P1 #7)

- `init_from` валидирует заголовок против `totalsize`: блоки struct/strings
  обязаны целиком помещаться в блоб, `totalsize` ограничен 4 МиБ;
- `walk()` ограничивает каждый токен, скан имени и prop-data концом
  struct-блока (и дополнительно клампится к totalsize);
- `cstr_at` сканирует NUL только внутри strings-блока
  (`G_STRINGS + G_STRINGS_SIZE`), выход за блок → "";
- `is_sedna`/`is_qemu` сканируют `min(totalsize, 256 KiB)`, а не фикс. 256 KiB.

Хост-тесты: `kernel/src/libfdt/tests.rs` (малформированные DTB, обрезанный
struct-блок, безымянный узел, отвергнутые заголовки).

## Тесты

- Хост: `cargo test -p onyx_kernel` —Creating/publish_ready-инварианты
  (`proc/tests.rs`), эксклюзивность UDP/CONNS/ARP + рекурсия NET_LOCK
  (`net/tests.rs`), политика chown (`fs/vfs/meta/chown.rs`), DTB-bounds
  (`libfdt/tests.rs`).
- QEMU SMP: `bash scripts/test_concurrency.sh` (2 и 4 харта: boot → login →
  fork-стресс фоновыми задачами → procfs-чтения → 0 паников).
