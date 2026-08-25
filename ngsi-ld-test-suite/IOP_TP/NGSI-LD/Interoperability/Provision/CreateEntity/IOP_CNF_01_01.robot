*** Settings ***
Documentation       Three brokers are set up A, B and C. A has two registrations, one inclusive for the entities created in B and one exclusive for the entity created in C. 
...                 Check that the entity is created in A and B but not in C.

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registrations
Test Teardown       Delete Entities and Delete Registrations

*** Variables ***
${entity_payload_filename}                        interoperability/offstreet-parking1-full.jsonld
${inclusive_registration_payload_file_path}       csourceRegistrations/interoperability/context-source-registration-inclusive-2.jsonld
${exclusive_registration_payload_file_path}       csourceRegistrations/interoperability/context-source-registration-exclusive-2.jsonld
${b1_url}
${b2_url}
${b3_url}

*** Test Cases ***
IOP_CNF_01_01 Create OffStreetParking:1
    [Documentation]    Pre-conditions: No user context. No data in any broker.
    ...                Registrations established: Inclusive in A to B. Exclusive in A to C.
    [Tags]    since_v1.6.1    iop    4_3_3    cf_06    additive-inclusive    proxy-exclusive    4_3_6    5_6_1

    #Create OffStreetParking:1 in A and check for a successful response
    ${expected_payload}=    Load Entity    ${entity_payload_filename}    ${entity_id}
    # The application/json retrieve below has no @context member; the loaded fixture carries one only so
    # it can be created. Drop it for the structural comparison.
    Remove From Dictionary    ${expected_payload}    @context
    # 5.6.1.4 (verbatim): for a matching exclusive registration where Create Entity is NOT
    # supported (this reg keeps the default read-only federationOps), "this shall result in an
    # error of type Conflict ... or in a partial success if some parts of it succeeded. The
    # matching Attributes are then removed from the Fragment and not processed further."
    # The inclusive forward to B and the local create succeed -> partial success = 207
    # Multi-Status (Table 6.4.3.1-1), and 'location' (exclusively owned by C) is NOT stored in A.
    Remove From Dictionary    ${expected_payload}    location
    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}    broker_url=${b1_url}
    Check Response Status Code    207    ${response.status_code}

    #Agent checks, with local=true, that the entity is created in A
    ${response}=    Retrieve Entity    ${entity_id}    local=true    broker_url=${b1_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()}    ${expected_payload}

    #Agent checks, with local=true, that the entity is created in B and only contains the attributes availableSpotsNumber and totalSpotsNumber
    ${response}=    Retrieve Entity    ${entity_id}    local=true    broker_url=${b2_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.json()}    availableSpotsNumber
    Should Contain    ${response.json()}    totalSpotsNumber
    Should Not Contain    ${response.json()}    name
    Should Not Contain    ${response.json()}    location

    #Agent checks, with local=true, that the entity is not created in C
    ${response}=    Retrieve Entity    ${entity_id}    local=true    broker_url=${b3_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}

*** Keywords ***
Setup Initial Context Source Registrations
    ${entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${entity_id}

    # The create must be FORWARDED to B, so the inclusive registration has to support the createEntity
    # operation. The default operation set is "federationOps" which is read-only (NGSI-LD 4.20), so
    # without this the create is never forwarded (NGSI-LD 5.6.1: forward only "in case the Create Entity
    # operation is supported").
    ${create_ops}=    Create List    createEntity
    ${registration_id1}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id1}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id1}
    ...    ${inclusive_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b2_url}
    ...    mode=inclusive
    ...    operations=${create_ops}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
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
    Delete Entity    ${entity_id}    broker_url=${b1_url}
