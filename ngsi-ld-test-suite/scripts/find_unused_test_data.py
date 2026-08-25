import os.path
from os import listdir
from os.path import isfile, isdir, dirname, abspath, join
from colorama import Fore, Style


def list_files_in_dir(test_suite_folder, data_folder):
    for filename in listdir(data_folder):
        aux = join(data_folder, filename)
        if isfile(aux):
            print(Style.RESET_ALL)
            print("Looking at test data file: ", filename)
            found = find_test_data_in_tc(dir_name=test_suite_folder, filename=filename)
            if not found:
                print(Fore.RED + "    Usage not found")

        else:
            list_files_in_dir(test_suite_folder=test_suite_folder, data_folder=aux)


def find_test_data_in_tc(dir_name, filename):
    found = False
    for file in listdir(dir_name):
        path = join(dir_name, file)
        if isdir(path):
            aux = find_test_data_in_tc(path, filename)
            found = aux or found
        else:
            if filename in open(path).read():
                found = True
                print(Fore.GREEN + "    Found usage in", os.path.relpath(path, join(base_folder, 'TP/NGSI-LD')))

    return found


if __name__ == '__main__':
    # Get the folder of the tests
    base_folder = dirname(dirname(abspath(__file__)))

    test_suite_folder = join(base_folder, "TP")
    data_folder = join(base_folder, 'data')

    list_files_in_dir(test_suite_folder=test_suite_folder, data_folder=data_folder)
