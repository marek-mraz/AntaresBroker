from os import listdir
from os.path import isdir, dirname, abspath, join
from robot import run


def find_test_data_in_tc(dir_name, filename, execute):
    for file in listdir(dir_name):
        path = dir_name + "/" + file
        if isdir(path):
            find_test_data_in_tc(path, filename, execute)
        else:
            if filename in open(path).read():
                if execute == "Y":
                    run(path)
                print(path)


if __name__ == '__main__':
    # Get the folder of the tests
    base_folder = dirname(dirname(abspath(__file__)))
    test_suite_folder = join(base_folder, "TP")

    test_data_file = input("Name of test data file to search for: ")
    run_matching_tc = input("Run matching Test Cases (Y/N)?: ")
    find_test_data_in_tc(dir_name=test_suite_folder, filename=test_data_file, execute=run_matching_tc)
