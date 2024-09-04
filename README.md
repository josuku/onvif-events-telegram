
# Onvif Events Telegram

This is a service to receive via Telegram messages the detections from a Onvif camera, indicating the detection time and a snapshot.

It was created to "extend" the availabilty of human detections generated by some chinese cameras which work with icSee software, using the Onvif protocol that these cameras implement.

Service is made using onvif-rs and a modified version of rustygram crate to permit send pictures (crate version only permits to send text messages, although send of csv files it is also implemented but not exposed)

## Use

- First you need is a Telegram Bot token to communicate with Telegram API. You can get it in @BotFather Telegram bot

- You also need the chat id of the Telegram user where you want to receive notifications. Using a Telegram bot like @username_to_id_bot to get it. Only chat ids which you configure will receive the detections

- Finally, you need the onvif camera config data: IP address with port, user, password and snapshot URI. This info could be obtained using examples of onvif-rs crate or with a mobile app which supports Onvif protocol. NOTE: in my case, default snapshot uris were not working because it says that credentials are not valid, although with same credentials throught RTSP they worked perfectly. To solve this, using Onvif Device Manager on Windows I could create a new user, and replacing the user and password of snapshot URI with the new user it works like a charm!

- Place this data into a copy of config.yaml.example file, removing the .example text

- to launch program:
```console
   $ git submodule update --init --recursive
   $ cargo run config.yaml
```
