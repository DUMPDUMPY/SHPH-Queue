SHPH Queue — ระบบเรียกคิว รพ.สต.
=================================

เริ่มใช้งาน
1. แตกไฟล์ zip ไว้ในโฟลเดอร์ที่ไม่ย้ายไปไหน เช่น C:\SHPH-Queue
2. ดับเบิลคลิก shph-queue.exe
   - ถ้า Windows ถาม Firewall ให้ติ๊ก "Private networks" แล้วกด Allow
   - ถ้า Windows SmartScreen เตือน ให้กด More info > Run anyway
3. หน้าต่างดำจะแสดงที่อยู่ เช่น http://192.168.1.10:8000
   เครื่องอื่นในวง LAN เปิดเบราว์เซอร์แล้วพิมพ์ที่อยู่นี้

หน้าเว็บ
  /display   จอทีวีให้คนไข้ดู
  /room/1    เครื่องในห้องตรวจ 1 (เลขห้องดูได้จากหน้าแรก)
  /manage    จัดการคิวทุกห้อง
  /admin     ตั้งค่า (รหัสผ่านเริ่มต้น: admin)

ไฟล์ในโฟลเดอร์
  shph-queue.exe          โปรแกรม (ปิดหน้าต่างดำ = ปิดระบบคิว)
  data\queue.db           ข้อมูลคิวและการตั้งค่า (สำรองไฟล์นี้)
  data\media\             รูปและวิดีโอที่อัปโหลดจากหน้า Admin
  voice\                  ไฟล์เสียงประกาศ
  install-autostart.bat   ให้โปรแกรมเปิดเองเมื่อเปิดเครื่อง
  uninstall-autostart.bat ยกเลิกการเปิดเอง
  open-firewall.bat       เปิดพอร์ต 8000 (คลิกขวา > Run as administrator)

ถ้าเครื่องอื่นเข้าไม่ได้
  - ตั้ง IP ของเครื่องนี้ให้คงที่ (Static IP) หรือจองไว้ที่เราเตอร์
  - รัน open-firewall.bat แบบ Run as administrator
  - ตั้งเครือข่ายเป็น Private network ใน Settings > Network & internet

เปลี่ยนพอร์ต: สร้าง shortcut แล้วเติม  --port 8080  ต่อท้าย Target
