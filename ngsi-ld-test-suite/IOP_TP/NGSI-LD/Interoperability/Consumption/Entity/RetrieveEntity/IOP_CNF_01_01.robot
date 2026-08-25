*** Settings ***
Documentation       Three brokers are set up A, B and C. A has two registrations, one inclusive for the entities created in B and one exclusive for the entity created in C. Check that the same entity created in B and C can be returned from A.

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
${inclusive_registration_payload_file_path}    csourceRegistrations/interoperability/context-source-registration-inclusive-2.jsonld
${exclusive_registration_payload_file_path}    csourceRegistrations/interoperability/context-source-registration-exclusive-2.jsonld
${b1_url}
${b2_url}
${b3_url}

*** Test Cases ***
IOP_CNF_01_01 Retrieve OffStreetParking:1
    [Documentation]    Pre-conditions: no user context. Data only on leaves. B contains OffStreetParking1 and OffStreetParking2. C contains OffStreetParking2.
    ...                Registrations established: Inclusive in A to B. Exclusive in A to C.
    [Tags]    since_v1.6.1    iop    4_3_3    cf_06    additive-inclusive    proxy-exclusive    4_3_6    5_7_1

    #Client retrieves OffStreetParking:1 in A and checks for a successful response.
    ${response}=    Retrieve Entity    ${entity_id1}    broker_url=${b1_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Should Contain   ${response.json()}    availableSpotsNumber
    Should Contain   ${response.json()}    totalSpotsNumber

    #Client retrieves OffStreetParking:1 in B.
    ${expected_payload}=    Load Entity    ${first_entity_payload_filename}    ${entity_id1}
    # The retrieved entity is rendered as application/json (no @context member); the loaded fixture
    # carries an @context only so it can be created. Drop it so the structural comparison matches.
    Remove From Dictionary    ${expected_payload}    @context
    ${response}=    Retrieve Entity    ${entity_id1}   broker_url=${b2_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}

    #Client checks that the entity returned is the full entity.
    Should Be Equal    ${response.json()}    ${expected_payload}

*** Keywords ***
Setup Initial Context Source Registrations
    ${entity_id1}=    Generate Random Parking Entity Id
    Set Suite Variable    ${entity_id1}
    ${response}=    Create Entity    ${first_entity_payload_filename}    ${entity_id1}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}
    
    ${entity_id2}=    Generate Random Parking Entity Id
    Set Suite Variable    ${entity_id2}
    ${response}=    Create Entity    ${second_entity_payload_filename}    ${entity_id2}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${second_entity_payload_filename}    ${entity_id2}    broker_url=${b3_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id1}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id1}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id1}
    ...    ${inclusive_registration_payload_file_path}
    ...    entity_id=${entity_id1}
    ...    broker_url=${b2_url}
    ...    mode=inclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${exclusive_registration_payload_file_path}
    ...    entity_id=${entity_id2}
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
    Delete Entity    ${entity_id1}    broker_url=${b2_url}
    Delete Entity    ${entity_id2}    broker_url=${b2_url}
    Delete Entity    ${entity_id2}    broker_url=${b3_url}