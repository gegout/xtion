# Firmware compatibility

| Firmware | Source | Hardware compatibility | Relevant fixes | Recommendation |
|---|---|---|---|---|
| 5.8.22 (2013-11-05) | [ASUS Xtion PRO firmware listing](https://www.asus.com/nl/supportonly/xtion%20pro/helpdesk_bios/) | Listed for PRO. GET_VERSION confirmed PS1080, FPGA 4, system 15 on our unit. ASUS supplies no finer PCB-revision matrix here. | ASUS mentions USB 3 controller-driver compatibility without identifying a Linux EP0 or stream-disable fix. | Retain the installed firmware. No update recommended. |

Confidence is high for the version, date, prohibition on downgrading and SDK package compatibility floor of 20.4.2.20. This is the latest exact-model release found. The [LIVE listing](https://www.asus.com/it/supportonly/xtion%20pro%20live/helpdesk_bios/) independently names 5.8.22. It does not establish interchangeable firmware images.

The [PRO downloads](https://www.asus.com/supportonly/xtion%20pro/helpdesk_download/) distinguish host package 20.4.4.0, OpenNI framework 1.5.2.23, Sensor DDK 5.1.0.41 and USB driver 3.1.3.1. None identifies a newer device firmware. The [manual catalogue](https://www.asus.com/supportonly/xtion%20pro/helpdesk_manual/) lists a shared PRO/LIVE developer appendix and quick-start guide. No supported downgrade or firmware repair for the observed timeout was identified.

The [ASUS model-specific BIOS API](https://www.asus.com/support/api/product.asmx/GetPDBIOS?website=global&model=Xtion%20PRO) returned one firmware file. The [quick-start PDF archive](https://dlcdnets.asus.com/pub/ASUS/MM/Xtion_Pro/Quick_Start_Guide.zip) is linked from PRO support but the PDF itself is titled Xtion PRO LIVE. Its USB cabling instructions do not identify this unit's depth format or audio configuration. The [developer appendix](https://dlcdnets.asus.com/pub/ASUS/MM/Xtion_Pro/Guidelines_For_Developers.zip) covers application design and publication, with no low-level USB recovery protocol.
