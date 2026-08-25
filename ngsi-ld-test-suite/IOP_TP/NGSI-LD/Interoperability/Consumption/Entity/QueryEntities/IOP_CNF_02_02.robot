*** Settings ***
Documentation       Four brokers are set up A, B, C and D. A has five registrations, two inclusive for the entities created in B, one redirect for the entities created in C and two redirect for the entity created in D.
...                 Check that the entities in B, C and D and said entities have the same attributes as the ones queried from A.
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registrations
Test Teardown       Delete Entities and Delete Registrations

*** Variables ***
${first_vehicle_payload_filename}                     interoperability/vehicle1-full.jsonld
${second_vehicle_payload_filename}                    interoperability/vehicle2-full.jsonld
${first_offstreet_payload_filename}                   interoperability/offstreet-parking1-full.jsonld
${second_offstreet_payload_filename}                  interoperability/offstreet-parking2-full.jsonld
${first_parking_location_name_payload_filename}       interoperability/offstreet-parking1-location-and-name.jsonld
${second_parking_location_name_payload_filename}      interoperability/offstreet-parking2-location-and-name.jsonld
${first_inclusive_registration_payload_file_path}     csourceRegistrations/interoperability/context-source-registration-inclusive-2.jsonld
${second_inclusive_registration_payload_file_path}    csourceRegistrations/interoperability/context-source-registration-inclusive-3.jsonld
${first_redirect_registration_payload_file_path}      csourceRegistrations/interoperability/context-source-registration-redirect-2.jsonld
${second_redirect_registration_payload_file_path}     csourceRegistrations/interoperability/context-source-registration-redirect-3.jsonld
${third_redirect_registration_payload_file_path}      csourceRegistrations/interoperability/context-source-registration-redirect-4.jsonld
${b1_url}
${b2_url}
${b3_url}
${b4_url}

*** Test Cases ***
IOP_CNF_02_02 Query Entities Of Type OffStreetParking And Vehicle with attrs
    [Documentation]    Pre-conditions: no user context. Data only on leaves. B contains OffStreetParking1 and Vehicle1. C contains OffStreetParking1 with location and name only and OffStreetParking2. D contains OffStreetParking2 with location and name only and Vehicle2.
    ...                Registrations established: Inclusive in A to B. Redirect in A to C. Redirect in A to D.
    [Tags]    since_v1.6.1    iop    4_3_3    cf_06    additive-inclusive    proxy-redirect    4_3_6    5_7_2    6_4_3_1

    #Client queries all entities with type OffStreetParking and Vehicle in A and checks for a successful response that contains the location attribute for all entities.
    ${response}=    Query Entities    entity_types=OffStreetParking,Vehicle    attrs=location    broker_url=${b1_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    &{payload}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${parking1_payload}=    Get From Dictionary    ${payload}    ${first_parking_entity_id}
    ${parking2_payload}=    Get From Dictionary    ${payload}    ${second_parking_entity_id}
    ${vehicle1_payload}=    Get From Dictionary    ${payload}    ${first_vehicle_entity_id}
    ${vehicle2_payload}=    Get From Dictionary    ${payload}    ${second_vehicle_entity_id}
    Should Contain    ${parking1_payload}    location
    Should Contain    ${parking2_payload}    location
    Should Contain    ${vehicle1_payload}    location
    Should Contain    ${vehicle2_payload}    location

    #Client queries all entities with type OffStreetParking and Vehicle in B, C and D. 
    ${response}=    Query Entities    entity_types=OffStreetParking,Vehicle    attrs=location    broker_url=${b2_url}    context=${ngsild_test_suite_context}
    &{first_expected_payload}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${response}=    Query Entities    entity_types=OffStreetParking,Vehicle    attrs=location    broker_url=${b3_url}    context=${ngsild_test_suite_context}
    &{second_expected_payload}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${response}=    Query Entities    entity_types=OffStreetParking,Vehicle    attrs=location    broker_url=${b4_url}    context=${ngsild_test_suite_context}
    &{third_expected_payload}=    Evaluate    {i['id']: i for i in ${response.json()}}

    #Client checks that the attributes of the entities in A are the same as the ones in B, C and D.
    ${expected_vehicle1}=    Get From Dictionary    ${first_expected_payload}    ${first_vehicle_entity_id}
    ${expected_parking1}=    Get From Dictionary    ${second_expected_payload}    ${first_parking_entity_id}
    ${expected_vehicle2}=    Get From Dictionary    ${third_expected_payload}    ${second_vehicle_entity_id}
    ${expected_parking2}=    Get From Dictionary    ${third_expected_payload}    ${second_parking_entity_id}
    Should Be Equal    ${vehicle1_payload}    ${expected_vehicle1}
    Should Be Equal    ${parking1_payload}    ${expected_parking1}
    Should Be Equal    ${vehicle2_payload}    ${expected_vehicle2}
    Should Be Equal    ${parking2_payload}    ${expected_parking2}

*** Keywords ***
Setup Initial Context Source Registrations
    ${first_parking_entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${first_parking_entity_id}
    ${second_parking_entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${second_parking_entity_id}
    ${first_vehicle_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${first_vehicle_entity_id}
    ${second_vehicle_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${second_vehicle_entity_id}

    ${response}=    Create Entity    ${first_vehicle_payload_filename}    ${first_vehicle_entity_id}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${first_offstreet_payload_filename}    ${first_parking_entity_id}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${first_parking_location_name_payload_filename}    ${first_parking_entity_id}    broker_url=${b3_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${second_offstreet_payload_filename}    ${second_parking_entity_id}    broker_url=${b3_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${second_parking_location_name_payload_filename}    ${second_parking_entity_id}    broker_url=${b4_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${second_vehicle_payload_filename}    ${second_vehicle_entity_id}    broker_url=${b4_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id1}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id1}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id1}
    ...    ${first_inclusive_registration_payload_file_path}
    ...    entity_id=${first_parking_entity_id}
    ...    broker_url=${b2_url}
    ...    mode=inclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${second_inclusive_registration_payload_file_path}
    ...    entity_id=${first_vehicle_entity_id}
    ...    broker_url=${b2_url}
    ...    mode=inclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id3}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id3}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id3}
    ...    ${first_redirect_registration_payload_file_path}
    ...    entity_id=${first_parking_entity_id}
    ...    broker_url=${b3_url}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id4}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id4}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id4}
    ...    ${second_redirect_registration_payload_file_path}
    ...    entity_id=${second_parking_entity_id}
    ...    broker_url=${b4_url}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id5}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id5}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id5}
    ...    ${third_redirect_registration_payload_file_path}
    ...    entity_id=${second_vehicle_entity_id}
    ...    broker_url=${b4_url}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    # Registrations propagate asynchronously to the broker's in-VM registry cache — an
    # immediate query/create can race ahead of the last registration (flaky aux/inclusive merges).
    Sleep    1s

Delete Entities And Delete Registrations
    Delete Context Source Registration    ${registration_id1}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id2}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id3}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id4}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id5}    broker_url=${b1_url}
    Delete Entity    ${first_parking_entity_id}    broker_url=${b2_url}
    Delete Entity    ${first_vehicle_entity_id}    broker_url=${b2_url}
    Delete Entity    ${first_parking_entity_id}    broker_url=${b3_url}
    Delete Entity    ${second_parking_entity_id}    broker_url=${b3_url}
    Delete Entity    ${second_vehicle_entity_id}    broker_url=${b4_url}
    Delete Entity    ${second_parking_entity_id}    broker_url=${b4_url}