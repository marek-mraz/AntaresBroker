*** Settings ***
Documentation       Four brokers are set up A, B, C and D. A has three registrations, one auxiliary for the entities created in B, one inclusive for the entities created in C and one inclusive for the entity created in D.
...                 Check that the entity returned from A match the attributes from the entity in B, C and D.
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registrations
Test Teardown       Delete Entities and Delete Registrations

*** Variables ***
${offstreet_no_location_payload_filename}             interoperability/offstreet-parking2-no-location.jsonld
${first_full_offstreet_payload_filename}              interoperability/offstreet-parking1-full.jsonld
${second_full_offstreet_payload_filename}             interoperability/offstreet-parking2-full.jsonld
${first_inclusive_registration_payload_file_path}     csourceRegistrations/interoperability/context-source-registration-inclusive-1.jsonld
${second_inclusive_registration_payload_file_path}    csourceRegistrations/interoperability/context-source-registration-inclusive-2.jsonld
${auxiliary_registration_payload_file_path}           csourceRegistrations/interoperability/context-source-registration-auxiliary-1.jsonld
${b1_url}
${b2_url}
${b3_url}
${b4_url}

*** Test Cases ***
IOP_CNF_03_01 Query Entities Of Type OffStreetParking Via GET
    [Documentation]    Pre-conditions: No user context. Data only on leaves. B contains OffStreetParking2 without location. C contains OffStreetParking1. D contains OffStreetParking1 without location.
    ...                Registration established: Auxiliary in A to B. Inclusive in A to C. Inclusive in A to D.
    [Tags]    since_v1.6.1    iop    4_3_3    cf_06    additive-inclusive    additive-auxiliary    4_3_6    5_7_2    6_4_3_1

    #Client queries all entities with type OffStreetParking in B, C and D (direct, no federation).
    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b2_url}    context=${ngsild_test_suite_context}
    ${payload}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${expected_parking1}=    Get From Dictionary    ${payload}    ${second_entity_id}
    Set Test Variable    ${expected_parking1}

    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b3_url}    context=${ngsild_test_suite_context}
    ${payload}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${expected_parking2}=    Get From Dictionary    ${payload}    ${entity_id}
    Set Test Variable    ${expected_parking2}

    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b4_url}    context=${ngsild_test_suite_context}
    ${payload}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${expected_parking3}=    Get From Dictionary    ${payload}    ${second_entity_id}
    Set Test Variable    ${expected_parking3}

    #Client queries A and checks the merged attributes match B, C and D. Registrations propagate
    #asynchronously to A's in-VM registry cache (and the stored-entityMap invalidation rides the
    #same channel), so poll until the distributed merge is consistent instead of sleeping.
    Wait Until Keyword Succeeds    10x    1s    Query A And Verify Merged Attributes

*** Keywords ***
Query A And Verify Merged Attributes
    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b1_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    &{payload}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${first_parking_payload}=    Get From Dictionary    ${payload}    ${entity_id}
    ${second_parking_payload}=    Get From Dictionary    ${payload}    ${second_entity_id}
    Should Be Equal    ${first_parking_payload}[name]    ${expected_parking2}[name]
    Should Be Equal    ${first_parking_payload}[location]    ${expected_parking2}[location]
    Should Be Equal    ${second_parking_payload}[availableSpotsNumber]    ${expected_parking3}[availableSpotsNumber]
    Should Be Equal    ${second_parking_payload}[totalSpotsNumber]    ${expected_parking3}[totalSpotsNumber]
    Should Not Be Equal    ${second_parking_payload}[availableSpotsNumber]    ${expected_parking1}[availableSpotsNumber]

Setup Initial Context Source Registrations
    ${entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${entity_id}
    ${second_entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${second_entity_id}

    ${response}=    Create Entity    ${offstreet_no_location_payload_filename}    ${second_entity_id}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${first_full_offstreet_payload_filename}    ${entity_id}    broker_url=${b3_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${second_full_offstreet_payload_filename}    ${second_entity_id}    broker_url=${b4_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id1}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id1}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id1}
    ...    ${auxiliary_registration_payload_file_path}
    ...    entity_id=${second_entity_id}
    ...    broker_url=${b2_url}
    ...    mode=auxiliary
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${first_inclusive_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b3_url}
    ...    mode=inclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id3}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id3}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id3}
    ...    ${second_inclusive_registration_payload_file_path}
    ...    entity_id=${second_entity_id}
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
    Delete Entity    ${second_entity_id}    broker_url=${b2_url}
    Delete Entity    ${entity_id}    broker_url=${b3_url}
    Delete Entity    ${second_entity_id}    broker_url=${b4_url}