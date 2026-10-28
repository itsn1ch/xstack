# Changelog

## Unreleased

- `worker split` kini membagi pekerjaan dalam batch maksimal 32 item. Thread yang kehabisan antrean mengambil separuh item yang belum dimulai dari thread lain, tanpa mengulang proses yang sedang berjalan. Batas concurrency, timeout per perintah, fail-fast, dan `--keep-going` tetap berlaku.
- Migrasikan seluruh tool `poteto-mode/scripts` ke Rust: `orch`, `watch-pr`, `check-plan`, dan `worktree-audit`. Pertahankan format store dan keluaran JSON, pindahkan tes perilaku ke Cargo, perbarui perintah instalasi dan playbook, serta hapus runtime dan konfigurasi Bun/Node.js.
- swarm step 4 kini hanya memakai hardware cap dan test slot untuk swarm verifikasi; tier dan budget cap hanya untuk slice penulisan kode poteto-mode, sesuai Subagents di poteto-mode. diving mengikuti aturan yang sama.
- diving wajib menuntaskan root cause: UNKNOWN hanya status kerja, setiap fakta yang hilang dikejar lewat loop resolusi dan eskalasi delapan langkah (kedalaman investigasi, bukan hardware); diagnosis selesai hanya dengan enam syarat; verdict REPRODUCED dan NOT REPRODUCED diganti BLOCKED untuk blocker objektif yang dibuktikan. Bug fix step 2 tidak lagi menyuruh membuat hipotesis.
- Sederhanakan diving untuk model non-frontier: prosedur tetap Locate, Trace, Compare, Isolate, Verify, History, Stop; tahap hipotesis dihapus; first wrong hop kembali jadi default dengan bisection berbatas; aturan missing operation; setiap level verifikasi dijalankan lewat swarm dengan batas hardware dan fallback.
- Refactor diving menjadi code-first: first violated behavioral boundary (first wrong hop untuk flow linear), causal graph untuk alur konkuren dan async, hipotesis dengan pengecekan pembeda termurah, verification ladder level 0 sampai 3 yang sadar hardware, verdict STATICALLY SUPPORTED, sumber kebenaran berlapis dengan konflik eksplisit, kondisi berhenti, dan tiga titik history yang dipisah.
- Hapus router xstack. Setiap skill dipanggil langsung, dan poteto-help kini memetakan to-spec dan to-evol. Jumlah skill menjadi 61.
- Tambahkan diving untuk mencari dan membuktikan penyebab bug: first wrong hop, yaitu hop pertama yang menyimpang dari source of truth, trace flow lengkap, repro, falsification, dan commit asal, dengan aturan bukti tanpa pengecualian. Bug fix step 2 memakainya. Jumlah skill menjadi 62.
- Refaktor to-evol menjadi alur SMALL/STANDARD/CRITICAL berbasis bukti: discovery bertahap, evidence map ringkas, klarifikasi hanya untuk keputusan blocking, pemeriksaan invariant/data lama/concurrency/recovery, batas berhenti eksplisit, dan verifikasi publikasi. Struktur spec serta integrasi to-spec/to-tickets/setup-xstack tetap; ID yang dipensiunkan tidak dipakai ulang dan keputusan blocking yang ditunda menjaga seluruh evolusi sebagai draft.
- setup-xstack mendeteksi code formatter per bahasa (script proyek, hook, lalu config), memasang formatter standar bila belum ada setelah disetujui, dan mencatatnya di `docs/agents/formatter.md` tanpa memformat ulang seluruh repo. poteto-mode wajib menjalankan formatter dan check-nya pada setiap file yang berubah sebelum commit, termasuk delegate.
- Tambahkan to-evol, turunan to-spec untuk fitur yang sudah jadi: context card dengan label Known, Inferred, Unknown dan grilling satu pertanyaan per pesan sebelum menulis; baseline dari spec, contract, dan kode; delta story New, Changed, Unchanged, Removed dengan ID tetap; kompatibilitas user, data, dan caller lama; serta dampak ke design contract. Spec lama tidak diubah. Jumlah skill menjadi 61.
- Gabungkan architect pstack ke to-tickets. to-tickets kini menulis design contract (vocabulary dengan kind dan source of truth, intent dan invariant per fungsi, flow berurutan), lalu mengimplementasikannya atau memecahnya menjadi tiket yang urutannya mengikuti flow. Perintah `/architect` diganti `/to-tickets`; baris setelan `architect runners` diganti `to-tickets runners`, dan baris lama tetap terbaca. Jumlah skill menjadi 60.
- poteto-mode mengestimasi tier waktu setiap task dan menentukan jumlah agent paralel otomatis dari budget dan hardware.
- Perkaya user story to-spec dengan ID stabil, aktor/manfaat, dasar requirement Confirmed atau Derived, preconditions, acceptance criteria Given/When/Then, dan dependensi antar-story yang relevan.
- Tambahkan pemeriksaan cakupan journey, izin/ownership, kegagalan, dan batasan tanpa mengarang fitur atau memenuhi kuota jumlah story.
- Pisahkan keputusan yang belum pasti dari requirement siap implementasi; pertahankan draft bila keputusan tersebut memblokir acceptance criteria yang dapat diverifikasi.

## 1.1.0

- Satukan setup model pstack dan konfigurasi tracker/domain proyek melalui setup-xstack.
- Ganti entrypoint setup lama dengan referensi model internal; jumlah skill tetap 61.
- Migrasikan router, bantuan, playbook model, dan panduan ke satu perintah setup.
- Hubungkan to-spec dan to-tickets ke konfigurasi tracker dan label/status yang sama, termasuk publikasi spec lokal.
- Pertahankan konfigurasi lama, batasi perubahan ke pilihan yang disetujui, dan verifikasi akses tracker tanpa membuat issue percobaan.

## 1.0.0

- Bundel seluruh 58 skill pstack versi lokal 0.15.32 beserta resource pendukung dan dua definisi subagent.
- Tambahkan hanya to-spec dan to-tickets dari koleksi skill Matt Pocock beserta metadata agen masing-masing.
- Tambahkan router xstack untuk memilih alur tanpa menjalankan tahap lanjutan secara otomatis.
- Ganti referensi setup-matt-pocock-skills pada dua skill pilihan dengan pembacaan konfigurasi tracker proyek dan pertanyaan terarah bila informasi publikasi belum tersedia.
- Sediakan manifest plugin Claude Code, Cursor, dan generik serta salinan kedua lisensi upstream.
