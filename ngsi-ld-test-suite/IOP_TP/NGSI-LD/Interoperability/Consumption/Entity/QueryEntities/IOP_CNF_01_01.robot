*** Settings ***
Documentation       Three brokers are set up A, B and C. A has two registrations, one inclusive for the entities created in B and one exclusive for the entity created in C. 
...                 The client sends a HTTP GET request to check that quering the offparking type returns the entities in B and C.
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registrations
Test Teardown       Delete Entities and Delete Registrations

*** Variables ***
${first_entity_payload_filename}               interoperability/offstreet-parking1-full.jsonld
${second_entity_payload_filename}              interoperability/offstreet-parking2-full.jsonld
${third_entity_payload_filename}               interoperability/offstreet-parking2-no-location-and-totalspotsnumber.jsonld
${inclusive_registration_payload_file_path}    csourceRegistrations/interoperability/context-source-registration-inclusive-2.jsonld
${exclusive_registration_payload_file_path}    csourceRegistrations/interoperability/context-source-registration-exclusive-2.jsonld
${b1_url}
${b2_url}
${b3_url}

*** Test Cases ***
IOP_CNF_01_01 Query Entities Of Type OffStreetParking Via GET
    [Documentation]    Pre-conditions: no user context. Data only on leaves. B contains OffStreetParking1 and OffStreetParking2 without location and totalSpotsNumber. C contains OffStreetParking2.
    ...                Registrations established: Inclusive in A to B. Exclusive in A to C.
    [Tags]    since_v1.6.1    iop    4_3_3    cf_06    additive-inclusive    proxy-exclusive    4_3_6    5_7_2    6_4_3_1

    #Agent queries all entities with type OffStreetParking in A and checks for a successful response that contains the attributes of both entities in B and C.
    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b1_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    @{payload}=    Set Variable   ${response.json()}
    # Registered attrs only (5.2.10/5.12): inclusive→B covers availableSpotsNumber+totalSpotsNumber,
    # exclusive→C covers location. Merged entity in A = union of those.
    ${merged}=    Evaluate    next(e for e in $payload if e['id'] == '${entity_id}')
    ${second_merged}=    Evaluate    next(e for e in $payload if e['id'] == '${second_entity_id}')
    Dictionary Should Contain Key    ${merged}    availableSpotsNumber
    Dictionary Should Contain Key    ${merged}    totalSpotsNumber
    Dictionary Should Contain Key    ${merged}    location
    Dictionary Should Contain Key    ${second_merged}    availableSpotsNumber

    #Agent queries all entities with type OffStreetParking in B and C
    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b2_url}    context=${ngsild_test_suite_context}
    @{first_expected_payload}=    Set Variable    ${response.json()}
    ${entity_ids}=    Create List    ${entity_id}    ${second_entity_id}
    Check Response Body Containing Entities URIS set to    ${entity_ids}    ${response.json()}

    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b3_url}    context=${ngsild_test_suite_context}
    @{second_expected_payload}=    Set Variable    ${response.json()}
    @{single_entity_id}=    Create List    ${entity_id}
    Check Response Body Containing Entities URIS set to    ${single_entity_id}    ${response.json()}

    #Agent checks that A's merged entity carries B's registered attribute values and C's registered location.
    ${b_entity}=    Evaluate    next(e for e in $first_expected_payload if e['id'] == '${entity_id}')
    ${c_entity}=    Evaluate    next(e for e in $second_expected_payload if e['id'] == '${entity_id}')
    Should Be Equal    ${merged}[availableSpotsNumber]    ${b_entity}[availableSpotsNumber]
    Should Be Equal    ${merged}[totalSpotsNumber]    ${b_entity}[totalSpotsNumber]
    Should Be Equal    ${merged}[location]    ${c_entity}[location]

*** Keywords ***
Setup Initial Context Source Registrations
    ${entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${entity_id}
    ${second_entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${second_entity_id}
    ${response}=    Create Entity    ${first_entity_payload_filename}    ${entity_id}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${third_entity_payload_filename}    ${second_entity_id}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${second_entity_payload_filename}    ${entity_id}    broker_url=${b3_url}
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
    ...    ${exclusive_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b3_url}
    ...    mode=exclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    # Registrations propagate asynchronously to the broker's in-VM registry cache — an
    # immediate query/create can race ahead of the last registration (flaky aux/inclusive merges).
    Sleep    1s

Delete Entities And Delete Registrations
    Delete Context Source Registration    ${registration_id1}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id2}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id3}    broker_url=${b1_url}
    Delete Entity    ${entity_id}    broker_url=${b2_url}
    Delete Entity    ${second_entity_id}    broker_url=${b2_url}
    Delete Entity    ${entity_id}    broker_url=${b3_url}