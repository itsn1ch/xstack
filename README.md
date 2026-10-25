# Xstack

Paket mandiri berisi **61 skill**:

- Seluruh workflow dari **58 skill pstack**, termasuk scripts, references, dan playbooks. Setup modelnya digabung ke `setup-xstack`; tidak ada perintah setup pstack terpisah.
- Hanya **to-spec** dan **to-tickets** dari `skills/skills/engineering/`. `to-spec` tetap skill sendiri dengan metadata `agents/openai.yaml`. Skill `architect` pstack digabung ke **to-tickets**, jadi to-tickets menulis design contract lalu memecahnya menjadi tiket berurutan atau langsung mengimplementasikannya.
- **to-evol**, turunan to-spec untuk mengembangkan fitur yang sudah jadi. Investigasi berbasis bukti dengan tingkat SMALL, STANDARD, atau CRITICAL; pertanyaan hanya untuk keputusan yang memblokir. Tetap menjaga baseline, delta story (New, Changed, Unchanged, Removed) dengan ID stabil, invariant, kompatibilitas user/data/caller lama, dampak design contract, dan verifikasi publikasi.
- **setup-xstack** sebagai satu-satunya alur setup, dan **poteto-help** untuk memilih skill yang cocok. Total 61 skill: setup lama diganti, architect digabung ke to-tickets, router xstack dihapus, serta to-evol dan diving ditambahkan.

Folder `agents/`, `docs/`, `assets/`, dan `automations/` pstack ikut disertakan untuk mempertahankan resource pendukung. Folder sumber `../pstack/` dan `../skills/` tidak diperlukan untuk menjalankan paket ini dan tidak diubah.

## Penggunaan

Panggil skill yang diinginkan langsung melalui harness. Bila belum tahu skill mana yang cocok, mulai dari [`/poteto-help`](skills/poteto-help/SKILL.md):

```text
/setup-xstack
/to-spec
/to-evol
/to-tickets
/poteto-mode implementasikan tiket yang sudah disetujui
/poteto-help
```

Prefix perintah mengikuti harness. Pada Claude Code sebagai plugin, gunakan namespace paket, misalnya `/xstack:setup-xstack`, `/xstack:to-spec`, `/xstack:to-evol`, `/xstack:to-tickets`, dan `/xstack:poteto-mode`.

## Satu kali setup

Jalankan [`/setup-xstack`](skills/setup-xstack/SKILL.md) di proyek tujuan. Satu alur ini mengatur:

- Model per role dan reasoning budget pstack, pada lokasi model yang sudah dibaca workflow pstack.
- Tracker proyek dan operasi publikasi, lokal Markdown atau integrasi tracker yang tersedia.
- Pemetaan label/status `ready-for-agent` yang dipakai `to-spec`, `to-evol`, dan `to-tickets`.
- Code formatter per bahasa. Setup mendeteksi formatter yang sudah dipakai proyek, lalu memasangnya bila belum ada dan disetujui. Hasilnya dicatat di `docs/agents/formatter.md`, dan `poteto-mode` wajib menjalankannya pada setiap file yang berubah sebelum commit.
- Pointer glossary dan ADR proyek.

Konfigurasi model dapat berlaku untuk seluruh user; tracker dan domain docs disimpan per proyek. Setup membaca konfigurasi yang sudah ada, menampilkan perubahan untuk disetujui, lalu menulis dan memverifikasi hasil. Menjalankan ulang mempertahankan pilihan yang tidak diminta berubah. Setup tidak membuat issue percobaan atau menjalankan implementasi.

Secara default konfigurasi proyek berada di `docs/agents/issue-tracker.md`, `docs/agents/triage-labels.md`, dan `docs/agents/domain.md`, dengan pointer dari satu file instruksi proyek. Konfigurasi Matt Pocock yang sudah ada dapat dipakai langsung. `to-spec`, `to-evol`, dan `to-tickets` membaca konfigurasi yang sama; bila belum tersedia, arahkan ke `/setup-xstack`. `to-spec` tetap mengonfirmasi testing seams; `to-evol` memakai seam yang ada tanpa konfirmasi rutin dan bertanya hanya bila keputusan memblokir hasil yang dapat diverifikasi. Persetujuan breakdown tiket tetap mengikuti `to-tickets`.

## Instalasi lokal

Dari direktori induk paket, periksa skill yang tersedia tanpa memasang:

```bash
bun x skills add ./xstack --list
```

Untuk memasang semua skill ke harness yang dipilih:

```bash
bun x skills add ./xstack --skill '*'
```

Jika memakai npm, ganti `bun x` dengan `npx`. Installer skills memasang skill, bukan mendaftarkan subagent atau automations plugin. Pertahankan folder paket ini agar resource di luar folder skill tetap dapat dijangkau; jangan pindahkan hanya satu `SKILL.md`. Untuk distribusi plugin, manifest tersedia di `.claude-plugin/plugin.json`, `.cursor-plugin/plugin.json`, dan `plugin.json`. Pemuatan plugin dan ketersediaan fitur pstack bergantung pada harness; fitur khusus Cursor tidak dijamin tersedia di harness lain.

## Sumber dan lisensi

- pstack lokal versi `0.15.32`, copyright 2026 Lauren Tan: [lisensi](licenses/pstack-LICENSE). Setup dan seluruh rujukannya diadaptasi menjadi `setup-xstack`; panduan terkait ikut diperbarui. Resource lain dapat tetap memakai nama upstream pstack.
- `to-spec` dan `to-tickets` (kini digabung dengan `architect` pstack di `skills/to-tickets/`), copyright 2026 Matt Pocock: [lisensi](licenses/matt-pocock-LICENSE). `to-evol` diturunkan dari `to-spec` dan memakai lisensi yang sama.
- Dari repository Matt Pocock hanya dua skill tersebut yang dibundel. Keduanya diadaptasi agar memakai konfigurasi setup-xstack, termasuk label/status kustom. Konvensi setup tracker dan domain diintegrasikan ke referensi setup-xstack tanpa menambah skill setup atau triage terpisah.

Format paket dirujuk melalui Context7 pada dokumentasi resmi [struktur plugin Claude Code](https://github.com/anthropics/claude-code/blob/main/plugins/plugin-dev/skills/plugin-structure/SKILL.md) dan [format skill](https://github.com/anthropics/claude-code/blob/main/plugins/plugin-dev/skills/skill-development/references/skill-creator-original.md).
