*** Settings ***
Documentation       Check that one can replace the entire content of a batch of entities on the Context Source thanks to a redirect registration

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup Entity Id And Registration And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server

*** Variables ***
${old_entity_payload_filename}              vehicle-simple-attributes.jsonld
${new_entity_payload_filename}              vehicle-simple-attributes-second.jsonld
${entity_pattern}                           urn:ngsi-ld:Vehicle:*
${registration_payload_file_path}           csourceRegistrations/context-source-registration-vehicle-batch-ops.jsonld

*** Test Cases ***
D013_01_red Batch Upsert Entities With Redirect Registration Without Update Flag
    [Documentation]    Check that if one requests the Context Broker to replace a batch of entities that match a redirect registration, these are replaced on the Context Source
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-redirect    4_3_6_3    5_6_8

    ${new_first_entity}=    Load Entity    ${new_entity_payload_filename}    ${first_entity_id}
    ${new_second_entity}=    Load Entity    ${new_entity_payload_filename}    ${second_entity_id}
    @{entities_to_be_upserted}=    Create List    ${new_first_entity}    ${new_second_entity}
    @{upserted_entities_ids}=    Create List    ${first_entity_id}    ${second_entity_id}

    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entityOperations/upsert    204
    Set Stub Reply    POST    /broker2/ngsi-ld/v1/entityOperations/upsert    204

    ${response}=    Batch Upsert Entities    @{entities_to_be_upserted}
    Check Response Status Code    204    ${response.status_code}

    Wait for redirected request
    ${request_payload}=    Get Request Body
    ${payload}=    Evaluate    json.loads('''${request_payload}''')    json
    # 6.3.5: the broker forwards application/json with the @context in the Link header; an
    # inline @context in a json body is BadRequestData on the receiver - compare without it.
    ${expected_first}=    Evaluate    {k: v for k, v in ${new_first_entity}.items() if k != '@context'}
    ${expected_second}=    Evaluate    {k: v for k, v in ${new_second_entity}.items() if k != '@context'}
    Should Contain    ${payload}    ${expected_first}
    Should Contain    ${payload}    ${expected_second}
    
    ${stub_count}=    Get Stub Count    POST    /broker1/ngsi-ld/v1/entityOperations/upsert
    Should Be Equal As Integers    ${stub_count}    1
    ${stub_count}=    Get Stub Count    POST    /broker2/ngsi-ld/v1/entityOperations/upsert
    Should Be Equal As Integers    ${stub_count}    1

*** Keywords ***
Setup Entity Id And Registration And Start Context Source Mock Server
    # 5.9.2.4: a redirect registration is refused with Conflict if ANY existing entity
    # matches it; this registration matches urn:ngsi-ld:Vehicle:* so residual Vehicles
    # from earlier tests would legally 409 it. Start from a clean local slate.
    Purge Entities    type=Vehicle    local=true    context=${ngsild_test_suite_context}
    ${first_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${first_entity_id}
    ${second_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${second_entity_id}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id_pattern=${entity_pattern}
    ...    mode=redirect
    ...    endpoint=/broker1
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${registration_payload_file_path}
    ...    entity_id_pattern=${entity_pattern}
    ...    mode=redirect
    ...    endpoint=/broker2
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Context Source Registration    ${registration_id2}
    Stop Context Source Mock Server