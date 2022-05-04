from urllib.parse import quote as urlencode
import requests
import sys
import argparse 

def eprint(*args, **kwargs):
    kwargs["file"] = sys.stderr
    print(*args, **kwargs)

p = argparse.ArgumentParser()
p.add_argument("doi", metavar="DOI")
args = p.parse_args()

doi = urlencode(args.doi)
# url = f"https://api.crossref.org/{urlencode(doi)}/transform"
url = f"https://doi.org/{doi}"
headers={"Accept": "application/vnd.citationstyles.csl+json"}

eprint(f"GET: {url}", end="")
r = requests.get(url, headers=headers)
eprint(f" [{r.status_code}]", )
print(r.text)
