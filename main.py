import requests
import time
import sys
from datetime import datetime
from fake_useragent import UserAgent

def create_session():
    session = requests.Session()
    session.headers = {
        'User-Agent': UserAgent().random,
        'Accept': 'text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8',
        'Accept-Language': 'en-US,en;q=0.5',
        'Accept-Encoding': 'gzip, deflate, br',
        'DNT': '1',
        'Connection': 'keep-alive',
        'Upgrade-Insecure-Requests': '1'
    }
    return session

def check_website(session, url):
    start_time = time.time()

    try:
        response = session.get(url, allow_redirects=True)
        response_time = time.time() - start_time
        response.raise_for_status()
        return True, '200 OK', response_time
    except requests.RequestException as e:
        response_time = time.time() - start_time
        return False, str(e), response_time

def log_status(url, is_up, details, response_time, log_file):
    timestamp = datetime.now().strftime('%Y-%m-%d %H:%M:%S')
    status_text = 'UP' if is_up else f'DOWN - {details}'
    log_entry = f"{timestamp} - {url} is {status_text}. Response time: {response_time:.2f} seconds\n"

    print(log_entry, end='')
    with open(log_file, 'a') as file:
        file.write(log_entry)

def main():
    url = input("Enter the URL of the website to monitor: ")
    log_file = input("Enter the log file name (e.g., website_status.log): ")
    interval_input = input("Enter the time interval between checks in seconds (e.g., 60): ")
    check_interval = int(interval_input)

    session = create_session()

    while True:
        try:
            is_up, details, response_time = check_website(session, url)
            log_status(url, is_up, details, response_time, log_file)
            time.sleep(check_interval)
        except KeyboardInterrupt:
            print("\nScript stopped by user. Exiting.")
            sys.exit()

if __name__ == "__main__":
    main()
