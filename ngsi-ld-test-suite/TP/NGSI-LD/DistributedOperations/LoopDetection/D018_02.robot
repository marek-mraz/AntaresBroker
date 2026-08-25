*** Settings ***
Documentation       Check Via Header Forwarding To Context Source across multiple NGSI-LD endpoints

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Registrations And Stop Context Source Mock Server

*** Variables ***
${first_entity_payload_filename}     vehicle-simple-attributes.jsonld
${second_entity_payload_filename}    vehicle-simple-attributes-second-different.jsonld
${third_entity_payload_filename}     vehicle-simple-attributes-second.json
${brandname_payload_filename}        vehicle-brandname-complete-fragment.jsonld
${entity_pattern}                    urn:ngsi-ld:Vehicle:*
${registration_payload_file_path}    csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld

*** Test Cases ***
D018_02_01 Check Post Via Header Forwarding To Context Source
    [Documentation]    Verify that the request contains the POST Via header when forwarding operations to the Context Source
    [Tags]    since_v1.8.1    dist-ops    4_3_3    cf_06    additive-inclusive    5_6_6    6_3_18

    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entities    201
    ${response}=    Create Entity    ${second_entity_payload_filename}    ${entity_id2}
    Check Response Status Code    201    ${response.status_code}

    Wait For Request
    ${response}=    Get Request Headers
    ${payload}=    Convert To Dictionary    ${response}
    Dictionary Should Contain Key    ${payload}    Via

D018_02_02 Check Update Via Header Forwarding To Context Source
    [Documentation]    Verify that the request contains the PATCH Via header when forwarding operations to the Context Source
    [Tags]    since_v1.8.1    dist-ops    4_3_3    cf_06    additive-inclusive    5_6_6    6_3_18

    Set Stub Reply    PATCH    /broker1/ngsi-ld/v1/entities/${entity_id}/attrs/    204
    ${response}=    Update Entity Attributes
    ...    ${entity_id}
    ...    ${brandname_payload_filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}

    Wait For Request
    ${response}=    Get Request Headers
    ${payload}=    Convert To Dictionary    ${response}
    Dictionary Should Contain Key    ${payload}    Via

D018_02_03 Check Delete Via Header Forwarding To Context Source
    [Documentation]    Verify that the request contains the DELETE Via header when forwarding operations to the Context Source
    [Tags]    since_v1.8.1    dist-ops    4_3_3    cf_06    additive-inclusive    5_6_6    6_3_18

    Set Stub Reply    DELETE    /broker1/ngsi-ld/v1/entities/${entity_id}    204
    ${response}=    Delete Entity    ${entity_id}
    Check Response Status Code    204    ${response.status_code}

    Wait For Request
    ${response}=    Get Request Headers
    ${payload}=    Convert To Dictionary    ${response}
    Dictionary Should Contain Key    ${payload}    Via

D018_02_04 Check Put Via Header Forwarding To Context Source
    [Documentation]    Verify that the request contains the PUT Via header when forwarding operations to the Context Source
    [Tags]    since_v1.8.1    dist-ops    4_3_3    cf_06    additive-inclusive    5_6_6    6_3_18

    Set Stub Reply    PUT    /broker1/ngsi-ld/v1/entities/${entity_id}    204
    ${response}=    Replace Entity
    ...    ${entity_id}
    ...    ${third_entity_payload_filename}
    ...    ${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}

    Wait For Request
    ${response}=    Get Request Headers
    ${payload}=    Convert To Dictionary    ${response}
    Dictionary Should Contain Key    ${payload}    Via

D018_02_05 Check Get Via Header Forwarding To Context Source
    [Documentation]    Verify that the request contains the GET Via header when forwarding operations to the Context Source
    [Tags]    since_v1.8.1    dist-ops    4_3_3    cf_06    additive-inclusive    5_6_6    6_3_18

    Set Stub Reply    GET    /broker1/ngsi-ld/v1/entities/${entity_id}    200
    ${response}=    Retrieve Entity    ${entity_id}
    Check Response Status Code    200    ${response.status_code}

    Wait For Request
    ${response}=    Get Request Headers
    ${payload}=    Convert To Dictionary    ${response}
    Dictionary Should Contain Key    ${payload}    Via

*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}
    ${entity_id2}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id2}
    ${response}=    Create Entity    ${first_entity_payload_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id_pattern=${entity_pattern}
    ...    mode=inclusive
    ...    endpoint=/broker1
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Registrations And Stop Context Source Mock Server
    Delete Entity    ${entity_id}
    Delete Entity    ${entity_id2}
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
