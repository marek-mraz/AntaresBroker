*** Settings ***
Documentation       Five brokers are set up A, B, C, D and E. A has two registrations, one auxiliary for the entities created in B and one inclusive for the entity created in C. B has two registration, one redirect for the entities created in D, one redirect for the entity created in E. C has one exclusive registration to E.
...                 The client creates the entity in A. The entity should be present in C, it should not be present in B, D and E. 

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registrations
Test Teardown       Delete Entities and Delete Registrations

*** Variables ***
${entity_payload_filename}                             interoperability/offstreet-parking1-full.jsonld
${auxiliary_registration_payload_file_path}            csourceRegistrations/interoperability/context-source-registration-auxiliary-2.jsonld
${first_redirect_registration_payload_file_path}       csourceRegistrations/interoperability/context-source-registration-redirect-1.jsonld
${second_redirect_registration_payload_file_path}      csourceRegistrations/interoperability/context-source-registration-redirect-2.jsonld
${inclusive_registration_payload_file_path}            csourceRegistrations/interoperability/context-source-registration-inclusive-1.jsonld
${exclusive_registration_payload_file_path}            csourceRegistrations/interoperability/context-source-registration-exclusive-1.jsonld
${b1_url}
${b2_url}
${b3_url}
${b4_url}
${b5_url}

*** Test Cases ***
IOP_CNF_04_01 Create OffStreetParking:1
    [Documentation]    Pre-conditions: No user context. No data in any broker.
    ...                Registrations established: Auxiliary in A to B. Inclusive in A to C. Redirect in B to D. Redirect in B to E. Exclusive in C to E.
    [Tags]    since_v1.6.1    iop    4_3_3    cf_06    additive-inclusive    additive-auxiliary    proxy-exclusive    proxy-redirect    4_3_6    5_6_1

    #Create the full entity of OffStreetParking:1 in A and check for a successful response
    # 5.6.1.4: C's exclusive reg to E keeps the default read-only federationOps, so C answers the
    # forwarded create with a partial success (Conflict for 'location', rest created) = 207.
    # A receives that 207 back from its inclusive forward to C and per Table 6.4.3.1-1 ("error
    # response received back from any distributed operation") reports 207 Multi-Status itself.
    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}    broker_url=${b1_url}
    Check Response Status Code    207    ${response.status_code}

    #Agent checks, with local=true, that the entity is created in A
    ${response}=    Retrieve Entity    ${entity_id}    local=true    broker_url=${b1_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    #Agent checks, with local=true, that the entity was not created in B
    ${response}=    Retrieve Entity    ${entity_id}    local=true    broker_url=${b2_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
    #Agent checks, with local=true, that the entity is created in C
    ${response}=    Retrieve Entity    ${entity_id}    local=true    broker_url=${b3_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    #Agent checks, with local=true, that the entity was not created in D
    ${response}=    Retrieve Entity    ${entity_id}    local=true    broker_url=${b4_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
    #Agent checks, with local=true, that the entity was not created in E
    ${response}=    Retrieve Entity    ${entity_id}    local=true    broker_url=${b5_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}

*** Keywords ***
Setup Initial Context Source Registrations
    ${entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${entity_id}
    ${create_ops}=    Create List    createEntity
    
    ${registration_id1}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id1}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id1}
    ...    ${auxiliary_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b2_url}
    ...    mode=auxiliary
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${first_redirect_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b4_url}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id3}=     Generate Random CSR Id
    Set Suite Variable    ${registration_id3}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id3}
    ...    ${second_redirect_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b5_url}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id4}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id4}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id4}
    ...    ${inclusive_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b3_url}
    ...    mode=inclusive
    # default operations = federationOps (4.20) excludes createEntity — declare it so the
    # create is distributed to C per the choreography (C's own exclusive reg to E keeps the
    # default ops, so C does NOT forward onward and E stays 404).
    ...    operations=${create_ops}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id5}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id5}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id5}
    ...    ${exclusive_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b5_url}
    ...    mode=exclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b3_url}
    Check Response Status Code    201    ${response.status_code}

    # Registrations propagate asynchronously to the broker's in-VM registry cache — an
    # immediate query/create can race ahead of the last registration (flaky aux/inclusive merges).
    Sleep    1s

Delete Entities And Delete Registrations
    Delete Context Source Registration    ${registration_id1}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id2}    broker_url=${b2_url}
    Delete Context Source Registration    ${registration_id3}    broker_url=${b2_url}
    Delete Context Source Registration    ${registration_id4}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id5}    broker_url=${b3_url}
    Delete Entity    ${entity_id}    broker_url=${b1_url}
    Delete Entity    ${entity_id}    broker_url=${b3_url}
