# Uptag examples

## Email report
With a cronjob and `mail` configured, you can send daily emails when there are updates available.

In `./email-report.sh.template`, fill out `<path to docker-compose.yml>`, `<email address>` and optionally the email subject which currently is `"Updates for Docker Services"`. Then configure a cronjob to run that script daily.

The script only sends you emails, if there are updates. An exit code of 0 means no updates, 1 means at least one compatible update, 2 means at least one breaking update. The body of the email will contain the standard `uptag` report, listing each update.