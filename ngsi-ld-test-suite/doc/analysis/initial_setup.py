import os
from os.path import dirname
import json
from os.path import dirname


class InitialSetup:
    def __init__(self):
        self.init = {
            'Setup Initial Context Source Registration': InitialSetup.init_csr(),
            'Initialize the Test Case': InitialSetup.init_csr(),
            'Create New Context Source Registration': InitialSetup.init_csr(),
            'Create Initial Context Source Registration': InitialSetup.init_csr(),
            'Initiate Test Case': InitialSetup.init_entity(),
            'Create Initial Entity': InitialSetup.init_entity2(),
            'Setup Initial Entity': InitialSetup.init_entity2(),
            'Create Initial Entity And Linked Entity': InitialSetup.init_entity_with_linked_entity(),
            'Create Initial Entity And Linked Entities': InitialSetup.init_entity_with_linked_entities(),
            'Create Initial Entities And Linked Entities': InitialSetup.init_entities_with_linked_entities(),
            'Initialize Environment': InitialSetup.init_entity2(),
            'Initialize Test': InitialSetup.init_entity2(),
            'Create Temporal Entity': InitialSetup.init_temporal_entity(),
            'Create Initial Temporal Entity': InitialSetup.init_temporal_entity2(),
            'Initialize Test Case': InitialSetup.init_temporal_entity2(),
            'Initialize Setup': InitialSetup.init_temporal_entity2(),
            'Create Id': InitialSetup.init_temporal_entity2(),
            'Create Initial Subscription': InitialSetup.init_subscription(),
            'Setup Initial Subscriptions': InitialSetup.init_subscription(),
            'Create Initial Subscription And Entity': InitialSetup.init_subscription_and_entity(),
            'Create Initial Subscription And Entity With Linked Entity': InitialSetup.init_subscription_and_entity_with_linked_entity(),
            'Start Mqtt Server And Connect': InitialSetup.init_mqtt_subscription(),
            'Setup Initial Entities': InitialSetup.init_entities(),
            'Setup Initial Temporal Entities': InitialSetup.init_temporal_entities(),
            'Create Initial Context Source Registration and Context Source Registration Subscription':
                InitialSetup.init_csr_and_sub(),
            'Create Initial Context Source Registrations And Context Source Registration Subscription':
                InitialSetup.init_csrs_and_sub(),
            'Setup Initial Context Source Registrations': InitialSetup.init_csrs(),
            'Setup Initial Context Source Registration Subscription': InitialSetup.init_csr_sub(),
            'Setup Initial Context Source Registration Subscriptions': InitialSetup.init_csr_subs(),
            'Create Initial set of @contexts': InitialSetup.create_set_contexts(),
            'Create Initial @context': InitialSetup.create_context(),
            'Create Initial cached @context': InitialSetup.created_cached_context(),
            'Create initial ImplicitlyCreated @context': InitialSetup.create_implictlycreated_context(),
            'Create Initial hosted @context': InitialSetup.create_hosted_context(),
            'Create Initial @context condition from an external server': InitialSetup.create_from_external_server(),
            'Delete core context and reload it': InitialSetup.delete_core_context(),
            'Setup Entity Id And Registration And Start Context Source Mock Server': InitialSetup.init_eid_and_csr_and_ms(),
            'Setup Registration And Start Context Source Mock Server': InitialSetup.init_csr_and_ms(),
            'Create Entity And Registration On The Context Broker And Start Context Source Mock Server': InitialSetup.init_local_entity_and_csr_and_ms(),
            'Create Initial Cached @context from entity': InitialSetup.init_cached_context(),
            'Create Entities And Registration And Start Context Source Mock Server': InitialSetup.init_entities_and_csr_and_ms(),
        }

        self.folder_test_suites = dirname(dirname(dirname(__file__)))

        self.total_files = -1
        self.files_with_setup = -1
        self.files_without_setup = -1

        self.code = list()
        self.files_with_setup = list()
        self.files_without_setup = list()

    def validate_setup_keys(self):
        """Validates that all setup keys used in robot files are defined in self.init.
        Should be called after all JSON files have been generated."""
        self.check_keys()

    @staticmethod
    def init_csr() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT contains a Context Source Registration 
        with id equal to ${context_source_registration_id}
        and payload set to ${context_source_registration_payload_file_path}
}'''

        return data

    @staticmethod
    def init_entity() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing an initial Entity ${entity} 
        with an id set to ${entityId} 
        and an attribute with an id set to ${atrId}
}'''
        return data

    @staticmethod
    def init_entity2() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing an initial Entity ${entity} 
        with an id set to ${entityId} 
}'''
        return data

    @staticmethod
    def init_entity_with_linked_entity() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing an initial Entity ${entity} 
        with an id set to ${linking_entity_id}
        and a linked entity with an id set to ${linked_entity_id}
}'''
        return data

    @staticmethod
    def init_entity_with_linked_entities() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing an initial Entity ${entity} 
        with an id set to ${linking_entity_id}
        and a linked entity with an id set to ${level_1_linked_entity_id}
        containing itself a linked entity with an id set to ${level_2_linked_entity_id}
}'''
        return data

    @staticmethod
    def init_entities_with_linked_entities() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing initial Entities ${first_entity_id} and ${second_entity_id}
        each one having a linked entity with an id respectively set to ${first_linked_entity_id} and ${second_linked_entity_id}
}'''
        return data

    @staticmethod
    def init_temporal_entity() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing an initial Temporal Entity ${entity} 
        with an id set to ${temporal_entity_representation_id} 
}'''
        return data

    @staticmethod
    def init_temporal_entity2() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing an initial Temporal Entity ${entity} 
        with an id set to ${temporal_entity_representation_id} 
}'''
        return data

    @staticmethod
    def init_subscription() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing an initial Subscription ${subscription} 
        with an id set to ${subscription_id} 
}'''
        return data

    @staticmethod
    def init_subscription_and_entity() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing an initial Subscription ${subscription} 
        with an id set to ${subscription_id}
        and an initial Entity ${entity}
        with an id set to ${entity_id}
}'''
        return data

    @staticmethod
    def init_subscription_and_entity_with_linked_entity() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing an initial Subscription ${subscription} 
        with an id set to ${subscription_id}
        and a linking Entity ${linking_entity}
        with an id set to ${linking_entity_id}
        and a linked Entity ${linked_entity}
        with an id set to ${linked_entity_id}
}'''
        return data

    @staticmethod
    def init_mqtt_subscription() -> str:
        data = '''with 
    the SUT being in the "initial state" and
    the SUT containing an initial Subscription ${subscription} 
        with an id set to ${subscription_id}
        and an notification endpoint set to a mqtt broker
}'''
        return data


    @staticmethod
    def init_entities() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    and containing a list of entities
}'''
        return data

    @staticmethod
    def init_temporal_entities() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing an a list of Temporal Entities 
}'''
        return data

    @staticmethod
    def init_csr_and_sub() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing a Context Source Registration (CSR1) providing latest information about some entities
    and the SUT containing a Context Source Registration Subscription (CSRS1)
}'''
        return data

    @staticmethod
    def init_csrs() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing a list of Context Source Registrations (CSRs) providing latest information about some entities
}'''
        return data

    @staticmethod
    def init_csrs_and_sub() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing a list of Context Source Registrations (CSRs) providing latest information about some entities
    and the SUT containing a Context Source Registration Subscription (CSRS1)
}'''
        return data

    @staticmethod
    def init_csr_sub() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing a Context Source Registration Subscription (CSRS1)
}'''
        return data

    @staticmethod
    def init_csr_subs() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing a set of Context Source Registration Subscriptions (CSRSs)
}'''
        return data

    @staticmethod
    def init_csr_and_server() -> str:
        data = '''with {
    the SUT containing a Context Source Registration of a context source (CS1) 
        providing temporal information of two entities of type Building between 2020-08-01T22:07:00Z and 2021-08-01T21:07:00Z
    and CS1 containing two temporal entities of type Building and temporal evolution of those entities in the mentioned interval.
}'''
        return data

    @staticmethod
    def init_csrs_subs() -> str:
        data = '''with {
    the SUT containing a Context Source Registration of a context source (CS1) 
        providing temporal information of two entities of type Building between 2020-08-01T22:07:00Z and 2021-08-01T21:07:00Z
    and CS1 containing two temporal entities of type Building and temporal evolution of those entities in the mentioned interval.
}'''
        return data

    @staticmethod
    def create_set_contexts():
        data = '''with {
    the SUT containing a set of three Hosted @contexts and the default Cached Core Context.
}'''
        return data

    @staticmethod
    def init_cached_context():
        data = '''with {
    the SUT containing a cached context from creating an entity.
}'''
        return data

    @staticmethod
    def create_context():
        data = '''with {
    the SUT containing a Hosted @context and the default Cached Core Context.
}'''
        return data

    @staticmethod
    def created_cached_context():
        data = '''with {
    the SUT containing a Cached @context added from a URL.
}'''
        return data

    @staticmethod
    def create_implictlycreated_context():
        data = '''with {
    the SUT containing a ImplicitlyCreated @context created from a subscription query.
}'''
        return data

    @staticmethod
    def create_hosted_context():
        data = '''with {
    the SUT containing a Hosted @context and the default Cached Core Context.
}'''
        return data

    @staticmethod
    def create_from_external_server():
        data = '''with {
    the SUT containing a Cached @context created from a entity creation through downloading from external server.
}'''
        return data

    @staticmethod
    def delete_core_context():
        data = '''with {
    the SUT containing a core context and it has been deleted with reload set to true.
}'''
        return data
    
    @staticmethod
    def init_eid_and_csr_and_ms() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing an initial Entity id set to ${entity_id}
    and the SUT containing a Context Source Registration 
        with id equal to ${registration_id}
        and payload set to ${registration_payload_file_path}
    and the SUT containing a Context Source Mock Server
}'''
        return data
    
    @staticmethod
    def init_csr_and_ms() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing a Context Source Registration 
        with id equal to ${registration_id}
        and payload set to ${registration_payload_file_path}
    and the SUT containing a Context Source Mock Server
}'''
        return data

    @staticmethod
    def init_local_entity_and_csr_and_ms() -> str:
        data = '''with {
    the SUT being in the "initial state" and
    the SUT containing an initial Entity ${entity} on the Context Broker
        with an id set to ${entity_id}
        and payload set to ${entity_payload_filename}
    and the SUT containing a Context Source Registration 
        with id equal to ${registration_id}
        and payload set to ${registration_payload_file_path}
    and the SUT containing a Context Source Mock Server
}'''
        return data

    @staticmethod
    def init_entities_and_csr_and_ms() -> str:
        data = '''with {
        the SUT being in the "initial state" and
        the SUT containing initial Entities ${first_entity_id} and ${second_entity_id} on the Context Broker
            with payload set to ${entity_payload_filename}
        and the SUT containing a Context Source Registration 
            with id equal to ${registration_id}
            and payload set to ${registration_payload_file_path}
        and the SUT containing a Context Source Mock Server
}'''
        return data

    def get_property_values(self, root_folder: str, property_name: str) -> [str, str]:
        robot_files_without_setup = list()
        robot_files_with_setup = list()
        self.total_files = 0

        for root, dirs, files in os.walk(root_folder):
            for file in files:
                if file.endswith(".json"):
                    file_path = os.path.join(root, file)
                    self.total_files = self.total_files + 1

                    with open(file_path, "r") as f:
                        try:
                            data = json.load(f)
                            # Skip files that are arrays or don't have test_cases key
                            if not isinstance(data, dict) or 'test_cases' not in data:
                                continue
                            value = [x[property_name] for x in data['test_cases']]

                            status = all(item == value[0] for item in value)

                            if not status:
                                print(f"{file_path} has different values of setup processes")
                            else:
                                value = value[0]

                            info = {
                                'file': file_path,
                                'data': value
                            }

                            if value is None:
                                robot_files_without_setup.append(file_path)
                            else:
                                robot_files_with_setup.append(info)
                        except (KeyError, TypeError, json.JSONDecodeError):
                            # Handle cases where the property is not found, wrong data type, or file is not valid JSON
                            pass

        return robot_files_with_setup, robot_files_without_setup

    def check_keys(self):
        self.generate_dictionaries()

        keys = self.init.keys()

        deleted_setup = [item for item in keys if item not in self.code]
        not_included_keys = [item for item in self.code if item not in keys]

        if len(deleted_setup) != 0 or len(not_included_keys) != 0:
            print('Checking the Setup functions...')
            if len(deleted_setup) != 0:
                print(f"    WARNING: Some of the Setup functions were deleted or not implemented:\n{deleted_setup}")

            if len(not_included_keys) != 0:
                print(f"    ERROR: Some Setup functions are not include in the Class:\n{not_included_keys}")

            print()

    def generate_dictionaries(self):
        folder = os.path.join(self.folder_test_suites, 'doc', 'results')
        attribute = "setup"

        self.files_with_setup, self.files_without_setup = self.get_property_values(folder, attribute)

        aux = [x['data'] for x in self.files_with_setup]
        self.code = list(set(aux))

        self.files_with_setup = len(self.code)
        self.files_without_setup = len(self.files_without_setup)

    def print_numbers(self):
        print("Show details of the Setup functions:")
        print(f"    Total number of Robot files: {self.total_files}\n"
              f"    Total number of Robot Files with Setup information: {self.files_with_setup}\n"
              f"    Total number of Robot Files without Setup information: {self.files_without_setup}")

        print()

    def get_setups(self) -> list:
        return self.code

    def get_robot_files_without_setup(self) -> list:
        return self.files_without_setup

    def get_initial_condition(self, initial_condition: str) -> str:
        try:
            if initial_condition is not None:
                data = self.init[initial_condition]
            else:
                data = '''with {
   the SUT containing an initial state
}'''
        except KeyError:
            print(f"ERROR: the initial condition '{initial_condition}' is not defined in the dictionary. "
                  f"Please check it and add the new initial condition.")
            data = '''with {
   the SUT containing an initial state
}'''

        return data


if __name__ == "__main__":
    instance = InitialSetup()

    instance.print_numbers()

    print(instance.get_initial_condition(initial_condition="Setup Initial Context Source Registration"))
    print(instance.get_initial_condition(initial_condition="Lorem Ipsum dolor sit"))
