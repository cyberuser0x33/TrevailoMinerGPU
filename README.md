<div align="right">
  <strong>Language:</strong>
  <a href="#english-version">English</a> | <a href="#русская-версия">Русский</a>
</div>

<h1 id="русская-версия"><img src="https://raw.githubusercontent.com/cyberuser0x33/TrevailoMinerGPU/main/logo.jpg" width="50"> Trevailo Coin CLI Miner ⛏️</h1>

Высокопроизводительный майнер для децентрализованной сети **Trevailo Coin** (TVC). Написан на языке Rust и предоставляет продвинутую гибридную архитектуру, разделяющую вычислительную нагрузку между центральным процессором (CPU) и видеокартой (GPU) через библиотеку [OpenCL](https://crates.io/crates/opencl3).

---
## Оригинальный проект взят [отсюда](https://github.com/makecooly-max/trevailo-miner.git)


## ⚡ Особенности и оптимизация

* **Hybrid Engine (CPU + GPU)**: Вычисления происходят параллельно! GPU берет на себя нижнюю половину диапазона `Nonce`, в то время как потоки CPU занимают верхнюю. Это позволяет использовать 100% мощности оборудования без коллизий.
* **Pre-hashing (Midstate Optimization)**: Блоки транзакций имеют длину 152 байта. CPU предварительно вычисляет промежуточное состояние SHA-256 (первые 128 байт данных) и передает аппаратному ускорителю только "хвост". Это снижает нагрузку на GPU-потоковые процессоры на **~66%**.
* **Развертывание циклов (Loop Unrolling)**: В OpenCL-ядре (C99) используются директивы `#pragma unroll 64` для полного снятия накладных расходов на циклы и ветвления. Никаких расхождений (branch divergence) на GPU.
* **Big-Endian Pre-formatting**: Перестановка байт в нужный порядок производится на CPU до отправки в видеопамять (VRAM), чтобы GPU работал исключительно с выровненными `u32` массивами ("сырыми" регистрами).

## Чек-лист проекта
- Системная поддержка 
  - (Windows) ✅
  - (Linux) ❌ (в процессе разработки)
- Языковой пакет ✅ 10+ языков включая (китайский и арабский)
- Оптимизированный код ✅
---

## 🛠️ Сборка из исходников (Windows)

Проект использует динамическое связывание `opencl3` и легко собирается на Windows, где установлен любой стандартный видеодрайвер. Тяжелый `OpenCL SDK` **не требуется** — необходимые библиотеки (`OpenCL.lib`) генерируются на лету или поставляются в папке.

### Требования
1. Установленный [Rust (rustup)](https://rustup.rs/)
2. Наличие C++ Build Tools (MSVC)

### Компиляция

1. Клонируйте репозиторий:
   ```bash
   https://github.com/cyberuser0x33/TrevailoMinerGPU.git
   cd trevailo-miner
   ```
2. *(Опционально)* Если у вас возникла ошибка `LNK1181: OpenCL.lib` при сборке вручную (зависит от видеокарты), просто запустите PowerShell-скрипт:
   ```powershell
   .\build_opencl_lib.ps1
   ```
   Скрипт найдет `OpenCL.dll` драйвера в вашей системе (System32) и сгенерирует `.lib` заглушку для компилятора в текущей папке.
3. Сборка релиза:
   ```bash
   cargo build --release
   ```
> **Совет для публикации (Maintainers):** Если вы хотите собрать `.exe` файл, который будет выложен в GitHub Releases для всех желающих, убедитесь, что в файле `.cargo/config.toml` ВЫКЛЮЧЕН флаг `"-C", "target-cpu=native"`. Этот флаг оптимизирует бинарник исключительно под "ваш" процессор (например, AVX2), что может вызвать ошибку `Illegal Instruction` у пользователей со старыми ПК. Для личного использования — включайте его для ультимативного 🚀 хешрейта!

## 🛠️ Сборка из исходников (Linux 🐧)

### ❗ Внимание: проект пока что не тестировался на Linux системах.
Теоретически можно собрать так:
(Ubuntu/Debian)


Установка зависимостей:
```bash
sudo apt update
sudo apt install build-essential ocl-icd-opencl-dev

```
Сборка релиза:
```bash
cargo build --release
```

## ⚙️ Использование и конфигурация

При первом запуске скомпилированного файла программа автоматически создаст файл **`config.json`**.
Остановите майнер (Ctrl+C), откройте `config.json` в любом текстовом редакторе и настройте его под себя:

```json
{
  "node_url": "http://31.131.21.11:8080",
  "wallet_address": "TxWz...Ваш_Адрес...",
  "threads": 8,
  "use_gpu": true,
  "language": "ru"
}
```

* `node_url` — Сетевой адрес ноды блокчейна Trevailo Coin. По умолчанию [официальный Mainnet сервер](https://github.com/makecooly-max/trevailo-wallet/blob/main/src/app.rs#L118).
* `wallet_address` — Кошелек, на который будут зачисляться добытые монеты TVC (награда за найденный блок).
* `threads` — Количество логических потоков CPU, выделяемых под майнинг. По умолчанию — все доступные. (Если система перегревается, уменьшите это значение).
* `use_gpu` — `true` включает параллельный майнинг на видеокарте (по умолчанию - true).
* `language` — Поддержка множества языков. Укажите `ru`, `uk`, `de`, `es`, `zh` и т.д. (Требует наличия файла `languagepack.json` в папке с майнером).
---
## Поддержать автора 💰
TrevailoCoin (TVC)
```
TxWz73zsah3z5m1fLcUvYM63JA8713ajy3
```
TRON (TRX)
```
TNetbbaT9S7Y5c55wnC1rnL67FUYDJq5nN
```
Solana (Sol)
```
FDzKuezC76ifJcbqZhL5Ej8z4XPe59sPcghn24hxgp4L
```
----
### 🔴 Дисклеймер (отказ от ответственности)

<img src="https://raw.githubusercontent.com/cyberuser0x33/cyberuser0x33.github.io/main/435345.webp" width="100">

**Автор проекта не является профессиональным кодером в написании низкоуровневого кода на C и майнинг программ. Весь код на С для работы майнера на видеокарте написан с помощью ИИ агентов. Поэтому в нем возможны баги, недочеты в оптимизации, и бог знает что еще. Автор не несёт ответственности за ваше оборудование, возможные баги и/или проблемы системы, видеокарты и прочего железа.**

---

## Счастливого майнинга! 🏆

<br><br><br>

<h1 id="english-version"><img src="https://raw.githubusercontent.com/cyberuser0x33/TrevailoMinerGPU/main/logo.jpg" width="50"> Trevailo Coin CLI Miner ⛏️</h1>

High-performance miner for the decentralized **Trevailo Coin** (TVC) network. Written in Rust, it provides an advanced hybrid architecture that distributes the computational load between the Central Processing Unit (CPU) and the Graphics Processing Unit (GPU) via the [OpenCL](https://crates.io/crates/opencl3) library.

---
## The original project is taken from [here](https://github.com/makecooly-max/trevailo-miner.git)


## ⚡ Features and Optimization

* **Hybrid Engine (CPU + GPU)**: Computations are performed in parallel! The GPU handles the lower half of the `Nonce` range, while CPU threads take the upper half. This allows using 100% of your hardware power without collisions.
* **Pre-hashing (Midstate Optimization)**: Transaction blocks are 152 bytes long. The CPU pre-computes the intermediate SHA-256 state (the first 128 bytes of data) and sends only the "tail" to the hardware accelerator. This reduces the load on GPU stream processors by **~66%**.
* **Loop Unrolling**: The OpenCL kernel (C99) uses `#pragma unroll 64` directives to completely eliminate loop and branching overhead. No branch divergence on the GPU.
* **Big-Endian Pre-formatting**: Byte swapping to the required order is performed on the CPU before sending data to the Video RAM (VRAM), so the GPU exclusively works with aligned `u32` arrays ("raw" registers).

## Project Checklist
- System support 
  - (Windows) ✅
  - (Linux) ❌ (in development)
- Language pack ✅ 10+ languages including (Chinese and Arabic)
- Optimized code ✅
---

## 🛠️ Building from Source (Windows)

The project uses dynamic linking for `opencl3` and is easily built on Windows with any standard video driver installed. The heavy `OpenCL SDK` is **not required** — the necessary libraries (`OpenCL.lib`) are generated on the fly or provided in the folder.

### Requirements
1. Installed [Rust (rustup)](https://rustup.rs/)
2. C++ Build Tools (MSVC) installed

### Compilation

1. Clone the repository:
   ```bash
   git clone https://github.com/Trevailo/trevailo-miner.git
   cd trevailo-miner
   ```
2. *(Optional)* If you get the `LNK1181: OpenCL.lib` error during manual build (depends on the graphics card), just run the PowerShell script:
   ```powershell
   .\build_opencl_lib.ps1
   ```
   The script will find the `OpenCL.dll` of the driver in your system (System32) and generate a `.lib` stub for the compiler in the current folder.
3. Build release:
   ```bash
   cargo build --release
   ```
> **Tip for publishing (Maintainers):** If you want to build an `.exe` file to be published in GitHub Releases for everyone, ensure that the `"-C", "target-cpu=native"` flag in the `.cargo/config.toml` file is TURNED OFF. This flag optimizes the binary exclusively for "your" processor (e.g., AVX2), which can cause an `Illegal Instruction` error for users with older PCs. For personal use — turn it on for ultimate 🚀 hashrate!

## 🛠️ Building from Source (Linux 🐧)

### ❗ Warning: the project has not yet been tested on Linux systems.
Theoretically, it can be built like this:
(Ubuntu/Debian)


Install dependencies:
```bash
sudo apt update
sudo apt install build-essential ocl-icd-opencl-dev

```
Build release:
```bash
cargo build --release
```

## ⚙️ Usage and Configuration

Upon the first launch of the compiled file, the program will automatically create a **`config.json`** file.
Stop the miner (Ctrl+C), open `config.json` in any text editor, and configure it for yourself:

```json
{
  "node_url": "http://31.131.21.11:8080",
  "wallet_address": "TxWz...Your_Address...",
  "threads": 8,
  "use_gpu": true,
  "language": "en"
}
```

* `node_url` — The network address of the Trevailo Coin blockchain node. The [official Mainnet server](https://github.com/makecooly-max/trevailo-wallet/blob/main/src/app.rs#L118) by default.
* `wallet_address` — The wallet to which mined TVC coins will be credited (reward for a found block).
* `threads` — The number of logical CPU threads allocated for mining. By default — all available. (If the system overheats, reduce this value).
* `use_gpu` — `true` enables parallel mining on the graphics card (true by default).
* `language` — Multilanguage support. Specify `en`, `ru`, `uk`, `de`, `es`, `zh`, etc. (Requires the `languagepack.json` file in the miner folder).
---
## Support the author 💰
TrevailoCoin (TVC)
```
TxWz73zsah3z5m1fLcUvYM63JA8713ajy3
```
TRON (TRX)
```
TNetbbaT9S7Y5c55wnC1rnL67FUYDJq5nN
```
Solana (Sol)
```
FDzKuezC76ifJcbqZhL5Ej8z4XPe59sPcghn24hxgp4L
```
----
### 🔴 Disclaimer

<img src="https://raw.githubusercontent.com/cyberuser0x33/cyberuser0x33.github.io/main/435345.webp" width="100">

**The author of the project is not a professional coder in writing low-level code in C and mining programs. All C code for the miner to run on the graphics card was written with the help of AI agents. Therefore, there may be bugs, optimization flaws, and god knows what else. The author is not responsible for your equipment, possible bugs and/or problems with the system, graphics card, and other hardware.**

---

## Happy mining! 🏆

