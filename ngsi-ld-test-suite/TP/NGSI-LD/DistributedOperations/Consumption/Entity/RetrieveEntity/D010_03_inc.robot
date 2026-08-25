*** Settings ***
Documentation       Verify that, when one has an inclusive registration on a Context Broker and an entity only on a Context Source, a retrieval request to the Context Broker the request is forwarded correcty to the Context Source using the appropriate operation

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Teardown       Delete Registration And Stop Context Source Mock Server
Test Template       Setup Registration And Context Source Mock Server And Retrieve Entity


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-complete.jsonld


*** Test Cases ***
D010_03_inc_01 Retrieve Entity By Id Using The retrieveEntity Operation
    [Documentation]    Check that if one retrieves entity living on a Context Source from a Context Broker, and the registration only allows the retrieveEntity operation, the request is forwarded as expected
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_7_1
    retrieveEntity    GET    /ngsi-ld/v1/entities/
D010_03_inc_02 Retrieve Entity By Id Using The queryEntity Operation
    [Documentation]    Check that if one retrieves entity living on a Context Source from a Context Broker, and the registration only allows the queryEntity operation, the request is forwarded as expected
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_7_1
    queryEntity    GET    /ngsi-ld/v1/entities?type=Vehicle
D010_03_inc_03 Retrieve Entity By Id Using The queryBatch Operation
    [Documentation]    Check that if one retrieves entity living on a Context Source from a Context Broker, and the registration only allows the queryBatch operation, the request is forwarded as expected
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_7_1
    queryBatch    POST    /ngsi-ld/v1/entityOperations/query


*** Keywords ***
Setup Registration And Context Source Mock Server And Retrieve Entity
    [Documentation]    Check that if one retrieves entity living on a Context Source from a Context Broker, and the registration only allows the requested operation, the request is forwarded as expected
    [Arguments]    ${operation}    ${method}    ${url}
    ${entity_id}=    Generate Random Vehicle Entity Id

    IF    '${operation}' == 'retrieveEntity'
        ${url}=    Set Variable    ${url}${entity_id}
    END

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${operations}=    Create List    ${operation}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    operations=${operations}
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    Start Context Source Mock Server

    ${entity_body}=    Load Entity    ${entity_payload_filename}    ${entity_id}
    Set Stub Reply    ${method}    ${url}    200    ${entity_body}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
