# Launch it using such a command: python3 scripts/export_results_csv.py ./results/output.xml results.csv

from robot.api import ExecutionResult, ResultVisitor
import sys

def substring_after_last_occurrence(path, char):
    s = str(path)
    index = s.rfind(char)
    if index == -1:
        return s

    return s[index + 1:]

class MyResultVisitor(ResultVisitor):
    def __init__(self):
        self.failed_tests = []
        self.passed_tests = []
        self.csv_file = csv_file

    def visit_test(self, test):
        if test.status == 'FAIL':
            test_result = {
                'name': test.name,
                'doc': test.doc,
                'suite': substring_after_last_occurrence(test.source,'/')
            }
            self.failed_tests.append(test_result)
        elif test.status == 'PASS':
            test_result = {
                'name': test.name,
                'doc': test.doc,
                'suite': substring_after_last_occurrence(test.source,'/')
            }
            self.passed_tests.append(test_result)

    def end_result(self, test_result):
        with open(self.csv_file, "w") as f:
            f.write("Test;Suite;Doc;Status\n")
            for test in self.passed_tests:
                f.write(f"{test['name']};{test['suite']};{test['doc']};PASS\n")
            for test in self.failed_tests:
                f.write(f"{test['name']};{test['suite']};{test['doc']};FAIL\n")


if __name__ == '__main__':
    try:
        output_file = sys.argv[1]
    except IndexError:
        output_file = "output.xml"
    try:
        csv_file = sys.argv[2]
    except IndexError:
        csv_file = "report.csv"
    result = ExecutionResult(output_file)
    result.visit(MyResultVisitor())