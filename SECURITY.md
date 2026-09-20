# Security

cu is a high-privilege local automation tool. Accessibility allows it to
inspect and control applications; Screen Recording allows it to capture
screens. An agent connected to cu may be able to read or change sensitive data.

Use cu only with trusted agent clients. Prefer type --stdin --secret for
credentials, review screenshots and logs before sharing them, and do not expose
an agent endpoint beyond the local machine.

Please report security issues privately to the repository owner rather than
opening a public issue with exploit details.
