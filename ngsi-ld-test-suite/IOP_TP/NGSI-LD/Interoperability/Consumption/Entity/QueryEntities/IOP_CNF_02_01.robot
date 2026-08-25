*** Settings ***
Documentation       Four brokers are set up A, B, C and D. A has three registrations, one inclusive for the entities created in B, one redirect for the entities created in C and D and one redirect for the entity created in D.
...                 The client sends a HTTP GET request to check that quering the offparking type returns the entities in B, C and D and said entities have the same attributes as the ones queried from A.
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registrations
Test Teardown       Delete Entities and Delete Registrations

*** Variables ***
${location_name_payload_filename}                     interoperability/offstreet-parking2-location-and-name.jsonld
${first_entity_no_location_payload_filename}          interoperability/offstreet-parking1-no-location.jsonld
${second_entity_no_location_payload_filename}         interoperability/offstreet-parking2-no-location.jsonld
${first_full_entity_payload_filename}                 interoperability/offstreet-parking1-full.jsonld
${second_full_entity_payload_filename}                interoperability/offstreet-parking2-full.jsonld
${inclusive_registration_payload_file_path}           csourceRegistrations/interoperability/context-source-registration-inclusive-2.jsonld
${first_redirect_registration_payload_file_path}      csourceRegistrations/interoperability/context-source-registration-redirect-2.jsonld
${second_redirect_registration_payload_file_path}     csourceRegistrations/interoperability/context-source-registration-redirect-3.jsonld
${b1_url}
${b2_url}
${b3_url}
${b4_url}

*** Test Cases ***
IOP_CNF_02_01 Query Entities Of Type OffStreetParking Via GET
    [Documentation]    Pre-conditions: no user context. Data on every broker. B contains OffStreetParking1 without location and OffStreetParking2 without location. C contains OffStreetParking1 and OffStreetParking2. D contains OffStreetParking2 with location and name only.
    ...                Registrations established: Inclusive in A to B. Redirect in A to C. Redirect in A to D.
    [Tags]    since_v1.6.1    iop    4_3_3    cf_06    additive-inclusive    proxy-redirect    4_3_6    5_7_2    6_4_3_1

    #Agent queries all entities with type OffStreetParking in A and checks for a successful response not containing the name attribute.
    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b1_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    # Responses are unordered — key by entity id instead of list position.
    &{payload}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${first_payload}=    Get From Dictionary    ${payload}    ${entity_id}
    ${second_payload}=    Get From Dictionary    ${payload}    ${second_entity_id}
    Should Not Contain    ${first_payload}   name
    Should Not Contain    ${second_payload}    name

    #Agent queries all entities with type OffStreetParking in B, C and D
    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b2_url}    context=${ngsild_test_suite_context}
    &{entities_b2}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${first_b2_payload}=    Get From Dictionary    ${entities_b2}    ${entity_id}
    ${second_b2_payload}=    Get From Dictionary    ${entities_b2}    ${second_entity_id}

    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b3_url}    context=${ngsild_test_suite_context}
    &{entities_b3}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${b3_payload}=    Get From Dictionary    ${entities_b3}    ${entity_id}

    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b4_url}    context=${ngsild_test_suite_context}
    &{entities_b4}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${b4_payload}=    Get From Dictionary    ${entities_b4}    ${second_entity_id}
    
    #Agent checks that OffStreetParking1 in A has the same availableSpotsNumber and totalSpotsNumber as the one in B and the same location attribute found in C. The OffStreetParking2 entity in A contains the attributes of both OffStreetParking2 availableSpotsNumber and totalSpotsNumber in C and the same location found in D.
    Should Be Equal    ${first_payload}[availableSpotsNumber]    ${first_b2_payload}[availableSpotsNumber]
    Should Be Equal    ${first_payload}[totalSpotsNumber]    ${first_b2_payload}[totalSpotsNumber]
    Should Be Equal    ${first_payload}[location]    ${b3_payload}[location]

    Should Be Equal    ${second_payload}[availableSpotsNumber]    ${second_b2_payload}[availableSpotsNumber]
    Should Be Equal    ${second_payload}[totalSpotsNumber]    ${second_b2_payload}[totalSpotsNumber]
    Should Be Equal    ${second_payload}[location]    ${b4_payload}[location]

*** Keywords ***
Setup Initial Context Source Registrations
    ${entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${entity_id}
    ${second_entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${second_entity_id}
    ${response}=    Create Entity    ${first_entity_no_location_payload_filename}    ${entity_id}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${second_entity_no_location_payload_filename}    ${second_entity_id}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${first_full_entity_payload_filename}    ${entity_id}    broker_url=${b3_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${second_full_entity_payload_filename}    ${second_entity_id}    broker_url=${b3_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${location_name_payload_filename}    ${second_entity_id}    broker_url=${b4_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id1}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id1}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id1}
    ...    ${inclusive_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b2_url}
    ...    mode=inclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${inclusive_registration_payload_file_path}
    ...    entity_id=${second_entity_id}
    ...    broker_url=${b2_url}
    ...    mode=inclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id3}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id3}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id3}
    ...    ${first_redirect_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b3_url}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id4}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id4}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id4}
    ...    ${first_redirect_registration_payload_file_path}
    ...    entity_id=${second_entity_id}
    ...    broker_url=${b3_url}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id5}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id5}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id5}
    ...    ${second_redirect_registration_payload_file_path}
    ...    entity_id=${second_entity_id}
    ...    broker_url=${b4_url}
    ...    mode=redirect
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
    Delete Entity    ${entity_id}    broker_url=${b2_url}
    Delete Entity    ${second_entity_id}    broker_url=${b2_url}
    Delete Entity    ${entity_id}    broker_url=${b3_url}
    Delete Entity    ${second_entity_id}    broker_url=${b3_url}
    Delete Entity    ${second_entity_id}    broker_url=${b4_url}