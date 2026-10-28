# Panduan singkat memakai Xstack

> Peringatan: alur ini termasuk tokenmaxxing, yaitu memakai banyak token untuk perencanaan, perbandingan solusi, dan review. Tentukan budget sebelum mulai.

Ikuti urutan ini: tentukan budget, susun spesifikasi, review arsitektur dan tiket, lalu jalankan implementasi dengan `/poteto-mode`.

## Tentukan budget dan model

Tetapkan batas biaya untuk pekerjaan Anda. Untuk menyesuaikan pemakaian token, pilih anggaran penalaran dan jumlah agent sesuai budget.

Sebagai pilihan awal, gunakan GPT untuk perencanaan dan DeepSeek untuk eksekusi. Pilih model yang tersedia di aplikasi agent Anda.

Jika Xstack belum terpasang, buka terminal di folder proyek. Untuk OpenCode, jalankan:

```bash
	npx skills add itsn1ch/xstack --skill '*' -a opencode -y
```

Atur proyek melalui agent:

```text
	/setup-xstack
```

Pilih model, anggaran penalaran, tempat menyimpan spesifikasi dan tiket, serta formatter. Periksa pengaturannya sebelum menyetujui perubahan, lalu mulai percakapan baru.

Jika memakai plugin Claude Code, gunakan awalan `/xstack:`, misalnya `/xstack:setup-xstack`.

## Susun spesifikasi dengan GPT

Pilih perintah sesuai pekerjaan Anda:

- Untuk fitur baru, gunakan `/to-spec` setelah kebutuhan dibahas dalam percakapan.
- Untuk perubahan fitur yang sudah ada, gunakan `/to-evol`. Jelaskan perlakuan terhadap data lama dan perilaku yang harus tetap sama.

Contoh untuk fitur baru:

```text
	/to-spec Susun spesifikasi fitur ekspor laporan penjualan ke CSV dari percakapan ini.
	Cantumkan hak akses, filter, kolom, format nilai, dan hasil saat tidak ada data.
	Tulis syarat selesai yang bisa diuji. Tandai keputusan yang belum disepakati.
```

Periksa spesifikasi sebelum melanjutkan. Selesaikan keputusan yang memengaruhi perilaku fitur.

## Rancang arsitektur dan tiket

Tetap gunakan GPT. Jalankan `/to-tickets` dengan spesifikasi yang sudah disetujui:

```text
	/to-tickets Rancang solusi dari spesifikasi yang sudah disetujui.
	Jelaskan batas modul, struktur data, dan sumber data yang menjadi acuan.
	Jelaskan function intent, yaitu tujuan setiap fungsi, beserta input dan outputnya.
	Tuliskan aturan yang harus selalu berlaku, alur eksekusi, dan penanganan kegagalan.
	Bagi pekerjaan menjadi tiket dengan urutan, dependensi, dan syarat selesai yang bisa diuji.
	Tampilkan rancangan dan usulan tiket untuk saya review.
	Jangan terbitkan tiket atau mulai implementasi sebelum saya menyetujuinya.
```

## Wajibkan review sebelum implementasi

Jangan lanjut ke eksekusi sebelum Anda menyetujui rancangan dan tiket. Periksa hal berikut:

- Pastikan arsitektur memenuhi kebutuhan dalam spesifikasi.
- Pastikan setiap modul dan fungsi punya tanggung jawab yang jelas.
- Pastikan input, output, dan aturan data tidak saling bertentangan.
- Pastikan alur mencakup kegagalan yang perlu ditangani.
- Pastikan urutan tiket mengikuti dependensi dan setiap tiket punya hasil yang bisa diuji.

Jika rancangan belum sesuai, minta revisi. Setelah review selesai, setujui rancangan dan minta agent menerbitkan tiket.

## Jalankan implementasi dengan DeepSeek

Pilih DeepSeek untuk eksekusi, lalu jalankan:

```text
	/poteto-mode Kerjakan tiket yang sudah disetujui sesuai urutannya.
	Ikuti rancangan dan tujuan fungsi yang sudah direview.
	Verifikasi setiap tiket dan tunjukkan hasil pengujiannya.
	Laporkan kendala dan bagian yang belum bisa diverifikasi.
```

Periksa bukti sesuai perubahan. Untuk bug, minta agent mengulang langkah yang memicu masalah. Untuk antarmuka, minta agent mencoba alurnya pada aplikasi yang berjalan.

Jika agent hanya menunjukkan build yang lulus, minta pemeriksaan perilaku:

```text
	Build sudah lulus. Jalankan alur yang berubah dan tunjukkan hasilnya.
```

Selesai jika syarat dalam spesifikasi terpenuhi dan bukti pemeriksaannya tersedia.

## Minta bantuan saat perlu

Jika jawaban sulit dipahami, ketik `/bro`. Untuk memilih skill sesuai masalah, gunakan `/poteto-help` beserta tujuan Anda.

Jika melanjutkan percakapan yang ditinggalkan, gunakan `/ketchup` untuk melihat perkembangan. Jika agent terus mengulangi kesalahan yang sama, gunakan `/correct`.
