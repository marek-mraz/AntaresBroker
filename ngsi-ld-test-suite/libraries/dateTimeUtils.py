from datetime import datetime
import re


def is_date(date, format):
    """Function exposed as a keyword to check whether the string can be interpreted as a date of given format
    :param date: string to check for date
    :param format: date format
    """
    try:
        datetime.strptime(date, format)
        return True
    except ValueError:
        return False


def parse_ngsild_date(date_string):
    """Function used in checks to assert if a received date is compliant with the NGSI-LD format
    :param date_string: string to check for date
    """
    try:
        # timestamp with fractional seconds, separated by a comma (v1.3+) or a
        # dot (v1.4+). RFC 3339 allows arbitrary fractional precision, and some
        # brokers emit nanoseconds (9 digits); Python's %f caps at 6, so the
        # fraction is truncated to microseconds before parsing.
        match = re.match(r"(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})[.,](\d+)Z", date_string)
        if match:
            seconds, fraction = match.group(1), match.group(2)[:6]
            return datetime.strptime(f"{seconds}.{fraction}Z", "%Y-%m-%dT%H:%M:%S.%fZ")

        # timestamp without fractional seconds
        match = re.match(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", date_string)
        if match:
            return datetime.strptime(date_string, "%Y-%m-%dT%H:%M:%SZ")

        # unknown timestamp format
        return None
    except ValueError:
        return None
