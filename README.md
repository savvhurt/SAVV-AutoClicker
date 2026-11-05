# autoclicker

Кроссплатформенный автокликер на Rust: **Linux** (Wayland и X11) и **Windows**.

Клики эмулируются на уровне ОС:

- **Linux** — виртуальная мышь через `/dev/uinput` (работает в Wayland и X11, root не нужен, если устройство доступно пользователю).
- **Windows** — `SendInput`, без драйверов и прав администратора.

## Возможности

- 0.2–1000 кликов/с, настраиваемая длительность нажатия и jitter.
- Левая, правая и средняя кнопка мыши.
- Фоновый демон + настраиваемая горячая клавиша: глобальная на Windows, через bind композитора на Linux.
- CLI: `toggle`, `on`, `off`, `status`, `quit`.
- Опциональный GUI (`autoclicker-gui`): запуск/остановка, живые настройки, статистика, запись горячей клавиши и сохранение профиля. Окно 460×560 (минимум 420×520), без прокрутки при штатных настройках.
- Кнопка гарантированно отпускается при остановке, Ctrl+C и завершении демона.
- Единственный экземпляр демона, управление через локальный сокет (Unix socket / Windows named pipe).

## Сборка

Только CLI:

```bash
cargo build --release
```

CLI и GUI:

```bash
cargo build --release --features gui
```

- Linux: `target/release/autoclicker`, `target/release/autoclicker-gui`
- Windows: `target\release\autoclicker.exe`, `target\release\autoclicker-gui.exe` (нужны Rust stable с MSVC toolchain и Visual Studio Build Tools)
- Для сборки GUI нужен Rust 1.92+ (требование `egui` 0.34); CLI собирается и на более старом Rust.

### Кросс-сборка под Windows из Linux

Можно собрать оба `.exe` без Windows-машины через MSVC-таргет и `cargo-xwin`. Нужны `clang` и `lld`:

```bash
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin
cargo xwin build --release --features gui --target x86_64-pc-windows-msvc
```

Готовые файлы появятся в `target/x86_64-pc-windows-msvc/release/`:

```text
autoclicker.exe
autoclicker-gui.exe
```

Первый запуск скачивает MSVC CRT и Windows SDK. GUI собирается как оконное приложение без консоли; при фатальной ошибке запуска пишется `%APPDATA%\autoclicker\gui-error.log` и показывается системное окно с ошибкой.

### Готовые Windows-сборки

В репозитории есть workflow **Windows release** (`.github/workflows/release.yml`):

- ручной запуск через **Actions → Windows release → Run workflow** собирает `autoclicker-windows-x86_64.zip` и прикладывает его как artifact;
- пуш тега `v*` дополнительно создаёт GitHub Release с этим архивом.

В архив входят CLI, GUI, README и лицензия шрифтов.

## Быстрый старт

Разовый запуск в терминале (остановка — Ctrl+C):

```bash
autoclicker run --cps 50 --button left --hold-ms 10
```

Демон:

```bash
autoclicker toggle          # запустить демон (если нужно) и включить клики
autoclicker toggle          # выключить
autoclicker status          # состояние и фактический CPS
autoclicker off             # выключить
autoclicker quit            # остановить демон
```

Включить с другими настройками:

```bash
autoclicker on --cps 100 --button right --hold-ms 8 --jitter-pct 10
autoclicker on --cps 25 --button middle
```

`on` и `toggle` сами поднимают демон. Без GUI настройки живут, пока работает демон; после перезапуска используются значения по умолчанию (`50 cps`, левая кнопка, `hold 10 ms`, без jitter).

Запуск демона в фоне:

```bash
autoclicker daemon --detach   # вернуть управление сразу
autoclicker daemon            # foreground, полезно для отладки
```

## GUI

```bash
./target/release/autoclicker-gui
```

![Минималистичная панель autoclicker](docs/gui.png)

На Windows:

```powershell
.\target\release\autoclicker-gui.exe
```

Интерфейс минималистичный: тёплый бумажный фон, чернильная типографика, один сигнальный оранжевый акцент, крупная кнопка питания и плоские ряды параметров без карточек и декоративной сетки. Шрифты IBM Plex Sans / IBM Plex Mono встроены в бинарь; лицензия лежит в `assets/fonts/LICENSE.txt`.

Что умеет:

- запускать и останавливать клики, поднимая демон при необходимости;
- менять скорость, кнопку, удержание и джиттер на лету, в том числе во время кликов;
- показывать фактический CPS, общее число кликов, период цикла и ошибку демона;
- записывать глобальную горячую клавишу на Windows и применять её без перезапуска демона;
- сохранять настройки между запусками и применять их к уже работающему демону;
- завершать демон отдельной кнопкой с подтверждением.

Окно рассчитано на 460×560 px и не прокручивается при штатных настройках; минимальный размер — 420×520. На Linux GUI показывает bind `MOD+SHIFT+C → TOGGLE`, который нужно прописать в композиторе (см. раздел «Горячая клавиша»). На Windows комбинация записывается прямо в интерфейсе: нажми на неё, затем Ctrl/Alt/Shift + клавиша; Esc отменяет запись. Конфликт с другой программой показывается под строкой, прежняя комбинация остаётся активной.

GUI ищет `autoclicker` рядом с `autoclicker-gui` или в `PATH`, поэтому держи оба бинаря вместе. При закрытии окна клики выключаются, но сам демон продолжает работать — чтобы остановить его, нажми «Завершить демон» или выполни `autoclicker quit`.

Профиль GUI (настройки и горячая клавиша):

- Linux: `$XDG_CONFIG_HOME/autoclicker/gui.toml` или `~/.config/autoclicker/gui.toml`
- Windows: `%APPDATA%\autoclicker\gui.toml`

Память: на Linux GUI занимает примерно 18–22 MB PSS / 25–30 MB RSS в покое; на Windows ориентир — 20–30 MB private working set. GUI рендерится программно (CPU), поэтому не тянет за собой GPU-драйверы и видеопамять.

## Linux

Проверить доступ к `/dev/uinput`:

```bash
test -w /dev/uinput && echo "uinput: writable"
```

Если доступа нет, загрузить модуль и добавить udev-правило:

```bash
sudo modprobe uinput
```

`/etc/udev/rules.d/99-uinput.rules`:

```udev
KERNEL=="uinput", SUBSYSTEM=="misc", MODE="0660", GROUP="input", TAG+="uaccess"
```

Затем:

```bash
sudo udevadm control --reload-rules
sudo udevadm trigger
```

После повторного входа в сессию `/dev/uinput` будет доступен. На CachyOS/niri доступ уже настроен через ACL; после обновления ядра или перезагрузки может потребоваться снова выполнить `sudo modprobe uinput`.

### Горячая клавиша

На Wayland глобальные клавиши обрабатывает композитор, поэтому `--hotkey` и запись комбинации в GUI на Linux не регистрируются. Настрой горячую клавишу средствами композитора; GUI показывает рекомендуемую комбинацию `MOD+SHIFT+C`.

**niri** — в `~/.config/niri/config.kdl`:

```kdl
binds {
    Mod+Shift+C { spawn "autoclicker" "toggle"; }
}
```

Если бинарь не в `PATH`, укажи полный путь:

```kdl
Mod+Shift+C { spawn "/home/vvv/autoclicker/target/release/autoclicker" "toggle"; }
```

**GNOME / KDE** — Settings → Keyboard → Custom Shortcut, команда:

```bash
autoclicker toggle
```

Клики идут в текущую позицию курсора, поэтому перед включением наведи курсор на нужное место.

## Windows

Запустить демон:

```powershell
.\autoclicker.exe daemon
```

Или скрыто, без удержания консоли:

```powershell
.\autoclicker.exe daemon --detach
```

По умолчанию глобальная горячая клавиша — **Ctrl+Alt+C**. Она переключает клики, пока демон запущен.

Комбинацию можно записать в GUI: она сохранится в `gui.toml` и применится к работающему демону без перезапуска. Если комбинация уже занята другой программой, демон вернётся к прежней и покажет ошибку.

Для CLI другая комбинация задаётся при старте демона:

```powershell
.\autoclicker.exe daemon --hotkey ctrl+shift+f8
```

Поддерживаются модификаторы `ctrl`, `alt`, `shift`, `win` и клавиши `A-Z`, `0-9`, `F1-F24`, `Space`, `Tab`, `Enter`, `Esc`, `Backspace`. В GUI записываются только Ctrl/Alt/Shift: клавишу Win egui не передаёт, но её по-прежнему можно указать через `--hotkey`.

Автозапуск: создай ярлык на `autoclicker.exe daemon --detach` в папке `shell:startup`. Консоль мелькнёт при входе, сам демон останется скрытым.

CLI работает так же, как на Linux:

```powershell
.\autoclicker.exe toggle
.\autoclicker.exe on --cps 100 --button left
.\autoclicker.exe status
.\autoclicker.exe off
.\autoclicker.exe quit
```

### Ограничения Windows

- `SendInput` не может кликать в окна, запущенные от администратора, если сам автокликер не elevated (UIPI).
- Некоторые игры и античиты игнорируют или блокируют синтетический ввод.
- При принудительном `TerminateProcess` кнопка может остаться зажатой: останавливай через `off`, `quit` или горячую клавишу. Короткий `hold` уменьшает последствия.
- Бинарь не подписан, SmartScreen может показать предупреждение.

## Параметры

| Параметр | По умолчанию | Описание |
|---|---:|---|
| `--cps N` | `50` | Кликов в секунду, `0.2–1000` |
| `--button left\|right\|middle` | `left` | Кнопка мыши |
| `--hold-ms MS` | `10` | Сколько кнопка держится нажатой, `0.1–5000`; автоматически ограничивается периодом |
| `--jitter-pct PCT` | `0` | Случайное отклонение интервала, `0–100` |
| `--duration-secs SECS` | — | Только для `run`: остановиться через N секунд |
| `--hotkey COMBO` | `ctrl+alt+c` | Только Windows |
| `--detach` | — | Только `daemon`: уйти в фон |

Примеры:

```bash
autoclicker run --cps 20 --hold-ms 25 --jitter-pct 15
autoclicker on --cps 200 --button left --hold-ms 2
autoclicker run --cps 5 --duration-secs 3
```

## Как это работает

- Планировщик использует абсолютные дедлайны `Instant`, поэтому задержки не накапливаются.
- Последние ~0.2 мс перед дедлайном (Linux) или ~1 мс (Windows) крутится spin-loop; на Windows дополнительно поднимается разрешение системного таймера до 1 мс.
- `hold` автоматически уменьшается, чтобы между отпусканием и следующим нажатием оставался зазор.
- Linux: uinput-устройство удаляется при завершении процесса, ядро само снимает зажатые кнопки.
- Windows: release вызывается на `off`/`quit`/Ctrl+C/панике через guard; при аварийном убийстве процесса гарантии нет.
- IPC: Unix socket с правами `0600` в `$XDG_RUNTIME_DIR` или Windows named pipe; файловый lock не даёт запустить второй демон.
- На Linux clicking работает и в нативных Wayland-приложениях, и в X11/XWayland.

## Ограничения

- Клик всегда идёт в текущую позицию курсора; выбора окна или координат нет.
- Реальный потолок CPS зависит от приложения/игры и их polling rate.
- На высоких CPS (сотни герц) ядро CPU может быть загружено из-за точного ожидания.
- Некоторые приложения и античиты блокируют uinput/SendInput.

## Разработка

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo xwin check --all-targets --all-features --target x86_64-pc-windows-msvc
```

CI (`.github/workflows/ci.yml`) собирает и тестирует CLI и GUI на `ubuntu-latest` и `windows-latest`.

Ручная проверка на Linux:

```bash
autoclicker run --cps 5 --duration-secs 2
```

Наведи курсор на текстовый редактор — должны появиться клики, а после завершения кнопка не должна остаться зажатой.
