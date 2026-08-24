# OnyxOS — Полный чеклист тестирования

Чекать по порядку после любых изменений. Отметки: ✅ проверено и зелёное,
⚠️ частично/известные ограничения, ❌ сломано, ⬜ не проверялось ещё.

---

## 1. Хост-тесты и сборки (быстрые, гонять всегда)

- [x] `cargo test -p onyx_core` — 58 passed (форматы, journal scan/replay, SHA-256 KAT)
- [x] `cargo test -p onyx_kernel --target x86_64-unknown-linux-gnu` — 104 passed
      (syscall ABI таблица, checksum, ARP, TCP verify, runqueue)
- [x] `cargo kbuild` — riscv64gc release, 0 warnings
- [x] `cargo kbuild --features smode` — S-mode сборка, 0 warnings
- [x] `cargo clippy -p onyx_kernel --release --target riscv64gc-unknown-none-elf` — 0 warnings
- [x] `cargo fmt --check` — весь workspace отформатирован
- [x] `cargo kbuild32` — riscv32imac собирается, CI-джоба переведена в строгую (d0afb9a)
- [x] `cargo tbuild` — host tools (mkimage, elf2onx, psfgen)

## 2. Сборка образа OnyxOS

- [x] `bash scripts/build-all.sh` — бутлоадер + ядро + shell + компилятор + диск
- [x] `bash scripts/mk-onyxfs-disk.sh` — диск собирается, tools находятся (path fallback)
- [x] `grep -abo ONY2 .build/onyx-boot-disk.img` → ровно один матч на offset **5242880**
      (p2 @ LBA 10240; если магия уехала — FAT/mtools испортили p2)
- [x] В manifest попали все бинарники: init, login, osh, passwd, useradd, userdel, vim
- [x] Файлы >40 КБ в образе читаются целиком (**mkimage single-indirect**, был баг:
      всё после 10 прямых блоков обрезалось — vim.onx грузился с нулевым текстом)

## 3. QEMU smoke (headless)

- [x] `bash scripts/qemu-smoke.sh` → PASS ×2 подряд (баннер, rootfs mount, userspace)
- [x] Тот же прогон с `-smp 2` (вручную) — без дедлоков
- [x] Проверить в логе: `hwrand: source=...` (virtio-rng > lcg-fallback),
      `grown ... blocks` при большом диске (grow-on-mount)

## 4. QEMU interactive smoke

Скрипт: `scripts/qemu-interactive-smoke.sh` (boot → login root/Enter → osh).

- [x] `[+] osh prompt OK` — дошло до шелла
- [x] `[+] echo OK` — команды исполняются
- [x] **0** строк `illegal instruction`
- [x] **2×** баннера `OnyxOS Login` (init перезапускает login после exit — respawn работает)
- [x] `whoami` → root; `uname`; `ls /bin`
- [ ] **Ctrl+C** во время долгой команды → процесс убит, код 130, osh жив
- [ ] **Backspace** в cooked-режиме затирает символ
- [ ] **Стрелки/tab** в raw-режиме osh (ESC-последовательности читаются одним read)
- [ ] **Ctrl+D = EOF**

## 5. Редактор vim (hard-float + большие файлы)

Скрипт-репро: `/tmp/opencode/vim_smoke.sh`.

- [x] `[+] vim UI rendered` — режимы NORMAL/INSERT рисуются
- [x] `[+] typed text visible` — ввод идёт
- [x] `:wq` сохраняет, `cat /tmp/note.txt` показывает текст
- [x] Пересборка локальным onyxCC даёт байт-идентичный vim.onx (детерминизм компилятора)

## 6. Альтернативные пути загрузки ядра

- [x] **OnyxBoot (M-mode→S-drop)** — основной путь, все смоки выше на нём
- [x] **SMP mailbox**: `-smp 2` → hart1 печатает вход в idle loop, ворк-стил жив;
      регрессия `-smp 1` отсутствует
- [x] **OpenSBI/fw_jump (S-mode)** — OC2R-путь: root cause был legacy virtio-mmio без
      GuestPageSize; фикс 8936890, fw_jump доводит до логина 3/3
- [x] S-mode ядро под M-mode OnyxBoot → честный kpanic с подсказкой (не тихая смерть на pc=0)
- [x] `rdcycle` из S-mode/U-mode легален (mcounteren/scounteren включены)
- [x] FP для U-mode включён (sstatus.FS=Initial) — hard-float бинарники не падают

## 7. Безопасность

- [x] Не-root `open /etc/shadow` → EPERM (любой режим открытия)
- [x] Ring-2 uid==0 может create/rename (passwd self-service контракт); обычный ring-2 — default-deny
- [x] `getentropy` через hwrand (Zkr→virtio-rng→LCG), источник виден в boot-логе
- [x] Предупреждение о деградации энтропии при генерации соли
- [x] Пароли: соль+10k SHA256, constant-time сравнение, формат `$5$salt$hash`,
      legacy-хэш мигрирует при следующей смене пароля
- [x] TCP: входящие сегменты матчатся по 4-tuple, checksum верифицируется
- [x] DHCP xid/chaddr сверяются; DNS txid случайный и сверяется
- [ ] Ручной пентест из игры: чужой L2-хост стучится в порты

## 8. Процессы/память (устойчивость)

- [x] Fork-bomb: >128 procs → EAGAIN; >32 procs на uid → EAGAIN
- [x] Бюджет user-pages 128 MiB → ENOMEM
- [x] Kstack canary: переполнение стека ядра ловится и репортится (`KERNEL STACK OVERFLOW`)
- [x] mmap overflow (len≈2^64) → Range, а не size=0
- [x] Невалидный user-указатель в write/read/getdents/ioctl → EFAULT, машина жива
      (репро: `write(1, 0x30000000, 1)` из ring-2 — раньше halt всей машины)
- [x] exit→wait: родитель не спит вечно, зомби переприживаются, login респавнится
- [x] IPC каналы под спинлоком; два send с разных хартов не рвут ringbuf

## 9. Файловая система

- [x] Journal recovery: clean / commit / torn tail / чужой magic (host-тесты)
- [ ] Journal «железный» цикл на OC2R: запись → kill VM → reboot → данные целы
- [x] Grow-on-mount: диск больше superblock → ФС расширяется (кап 128 MiB bitmap)
- [x] FAT32: зеркало второй FAT получает патченные записи; FAT16 отклоняется
- [ ] Snapshot/rollback на несъёмном диске OC2R

## 10. Сеть

- [ ] ping/ifconfig утилиты в osh (ещё нет)
- [ ] DNS-резолвер + /etc/hosts
- [ ] virtio-net discovery через FDT (сейчас захардкожены QEMU MMIO адреса)
- [ ] DHCP на реальном sedna (не только QEMU user-net)

## 11. Дисплей / ввод OC2R

- [ ] put_pixel глифов по MMIO на 1920×1080 — приемлемая скорость
      (scroll/clear уже word-копии; сам glyph-blit пока попиксельный)
- [ ] Tearing/двойная буферизация
- [ ] Палитра fb_term для 16bpp консистентна с UART
- [ ] Выбор simple-framebuffer ноды через bootargs (монитор vs проектор)
- [ ] mmap /dev/fb0 из юзерспейса

## 12. Релизный бандл OC2R (/storage/project/jvm/oc2r)

- [x] Ресурсы обновлены: onyx-kernel.bin (**smode** build!), onyxfs.img (ONY2 @0, vim внутри)
- [x] compileJava проходит
- [x] **Ядро стартует под модовым fw_jump.bin** — фикс вошёл в бандл (oc2r 646dbbd)
- [x] Пуш oc2r (work → origin/work)
- [ ] Полный игровой цикл: give flash → вставить → загрузка → login → vim → save (нужна игра)

## 13. CI/CD (все репозитории)

- [x] OnyxKernel: ci.yml (MSRV-pin, fmt/clippy/tests, riscv64 matrix, smode) — зелёный
- [x] OnyxCompiller: build + selfhost-test (fortify-safe strncpy) — зелёный
- [x] OnyxBoot: cross-compile workflow + entry smoke — зелёный
- [x] OnyxShell: существующий ci.yml здоров — зелёный
- [x] Onyx-Vim: syntax-check против libonyxc + ONX1 artifact-check — зелёный
- [x] OnyxOS: Build & Test (6 джоб) + Release — зелёные
- [ ] riscv32 джоба перевести из advisory в строгую (после фикса таргета)

---

Правило: любой фикс сопровождается прогоном секций 1–4 минимум; изменения
загрузчика/энтропии/ФС — плюс соответствующая специализированная секция.
